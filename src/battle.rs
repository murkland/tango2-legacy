use crate::datachannel;
use crate::input;
use crate::mgba;
use crate::protocol;
use crate::signor;
use prost::Message;
use rand::Rng;
use rand::SeedableRng;
use sha3::digest::ExtendableOutput;
use std::io::Read;
use std::io::Write;
use subtle::ConstantTimeEq;

pub struct Init {
    input_delay: u32,
    marshaled: [u8; 0x100],
}

pub struct BattleState {
    pub number: u32,
    pub battle: Option<Battle>,
    won_last_battle: bool,
}

enum Negotiation {
    NotReady,
    Negotiated {
        dc: std::sync::Arc<datachannel::DataChannel>,
        rng: rand_pcg::Mcg128Xsl64,
    },
    Err(anyhow::Error),
}

pub struct Match {
    negotiation: tokio::sync::Mutex<Negotiation>,
    cancellation_token: tokio_util::sync::CancellationToken,
    session_id: String,
    match_type: u16,
    game_title: String,
    game_crc32: u32,
    battle_state: tokio::sync::Mutex<BattleState>,
    aborted: std::sync::atomic::AtomicBool,
}

fn make_rng_commitment(nonce: &[u8]) -> anyhow::Result<[u8; 32]> {
    let mut shake128 = sha3::Shake128::default();
    shake128.write_all(b"syncrand:nonce:")?;
    shake128.write_all(&nonce)?;

    let mut commitment = [0u8; 32];
    shake128
        .finalize_xof()
        .read_exact(commitment.as_mut_slice())?;

    Ok(commitment)
}

impl Match {
    pub fn new(session_id: String, match_type: u16, game_title: String, game_crc32: u32) -> Self {
        Match {
            negotiation: tokio::sync::Mutex::new(Negotiation::NotReady),
            cancellation_token: tokio_util::sync::CancellationToken::new(),
            session_id,
            match_type,
            game_title,
            game_crc32,
            battle_state: tokio::sync::Mutex::new(BattleState {
                number: 0,
                battle: None,
                won_last_battle: false,
            }),
            aborted: false.into(),
        }
    }

    pub fn abort(&self) {
        self.aborted
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn aborted(&self) -> bool {
        self.aborted.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub async fn lock_battle_state(&self) -> tokio::sync::MutexGuard<'_, BattleState> {
        self.battle_state.lock().await
    }

    pub async fn negotiate(&self) -> anyhow::Result<()> {
        let mut sc = signor::Client::new("http://localhost:12345").await?;

        let api = webrtc::api::APIBuilder::new().build();
        let (peer_conn, dc, side) = sc
            .connect(
                || async {
                    let peer_conn = api
                        .new_peer_connection(
                            webrtc::peer_connection::configuration::RTCConfiguration {
                                ..Default::default()
                            },
                        )
                        .await?;
                    let dc = peer_conn
                        .create_data_channel(
                            "tango",
                            Some(
                                webrtc::data_channel::data_channel_init::RTCDataChannelInit {
                                    id: Some(1),
                                    negotiated: Some(true),
                                    ordered: Some(true),
                                    ..Default::default()
                                },
                            ),
                        )
                        .await?;
                    Ok((peer_conn, dc))
                },
                &self.session_id,
            )
            .await?;
        let dc = datachannel::DataChannel::new(dc).await;

        // TODO: Other negotiation stuff.
        log::info!(
            "local sdp: {}",
            peer_conn.local_description().await.unwrap().sdp
        );
        log::info!(
            "remote sdp: {}",
            peer_conn.remote_description().await.unwrap().sdp
        );

        let mut nonce = [0u8; 16];
        rand::rngs::OsRng {}.fill(&mut nonce);
        let commitment = make_rng_commitment(&nonce)?;

        log::info!("our nonce={:?}, commitment={:?}", nonce, commitment);

        dc.send(
            protocol::Packet {
                which: Some(protocol::packet::Which::Hello(protocol::Hello {
                    protocol_version: protocol::VERSION,
                    game_title: self.game_title.clone(),
                    game_crc32: self.game_crc32,
                    match_type: self.match_type as u32,
                    rng_commitment: commitment.to_vec(),
                })),
            }
            .encode_to_vec()
            .as_slice(),
        )
        .await?;

        let hello = match protocol::Packet::decode(
            match dc.receive().await {
                Some(d) => d,
                None => anyhow::bail!("did not receive packet from peer"),
            }
            .as_slice(),
        )? {
            protocol::Packet {
                which: Some(protocol::packet::Which::Hello(hello)),
            } => hello,
            p => {
                anyhow::bail!("expected hello, got {:?}", p)
            }
        };

        log::info!("their hello={:?}", hello);

        if commitment.ct_eq(hello.rng_commitment.as_slice()).into() {
            anyhow::bail!("peer replayed our commitment")
        }

        if hello.protocol_version != protocol::VERSION {
            anyhow::bail!(
                "protocol version mismatch: {} != {}",
                hello.protocol_version,
                protocol::VERSION
            );
        }

        if hello.match_type != self.match_type as u32 {
            anyhow::bail!(
                "match type mismatch: {} != {}",
                hello.match_type,
                self.match_type
            );
        }

        if hello.game_title[..8] != self.game_title[..8] {
            anyhow::bail!("game mismatch: {} != {}", hello.game_title, self.game_title);
        }

        dc.send(
            protocol::Packet {
                which: Some(protocol::packet::Which::Hola(protocol::Hola {
                    rng_nonce: nonce.to_vec(),
                })),
            }
            .encode_to_vec()
            .as_slice(),
        )
        .await?;

        let hola = match protocol::Packet::decode(
            match dc.receive().await {
                Some(d) => d,
                None => anyhow::bail!("did not receive packet from peer"),
            }
            .as_slice(),
        )? {
            protocol::Packet {
                which: Some(protocol::packet::Which::Hola(hola)),
            } => hola,
            p => {
                anyhow::bail!("expected hello, got {:?}", p)
            }
        };

        log::info!("their hola={:?}", hola);

        if !bool::from(make_rng_commitment(&hola.rng_nonce)?.ct_eq(hello.rng_commitment.as_slice()))
        {
            anyhow::bail!("failed to verify rng commitment")
        }

        log::info!("connection ok!");

        let seed = hola
            .rng_nonce
            .iter()
            .zip(nonce.iter())
            .map(|(&x1, &x2)| x1 ^ x2)
            .collect::<Vec<u8>>();

        let mut rng = rand_pcg::Mcg128Xsl64::from_seed(seed.try_into().unwrap());

        self.battle_state.lock().await.won_last_battle =
            rng.gen::<bool>() == (side == signor::ConnectionSide::Polite);
        *self.negotiation.lock().await = Negotiation::Negotiated { dc, rng };
        Ok(())
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        let cancellation_token = self.cancellation_token.clone();
        tokio::select! {
            _ = cancellation_token.cancelled() => {
                let negotiation = self.negotiation.lock().await;
                match &*negotiation {
                    Negotiation::Negotiated { dc, .. } => {
                        let _ = dc.close().await;
                    },
                    _ => {},
                };
                anyhow::bail!("match cancelled");
            },
            r = async {
                self.negotiate().await?;
                let dc = match &*self.negotiation.lock().await {
                    Negotiation::Negotiated { dc, .. } => dc.clone(),
                    _ => unreachable!(),
                };

                loop {
                    match match protocol::Packet::decode(match dc.receive().await {
                        None => break,
                        Some(buf) => buf,
                    }.as_slice())?.which {
                        None => break,
                        Some(b) => b,
                    } {
                        protocol::packet::Which::Init(init) => {

                        },
                        protocol::packet::Which::Input(input) => {
                            let mut battle_state = self.battle_state.lock().await;
                            if input.battle_number != battle_state.number {
                                log::info!("battle number mismatch, dropping input");
                                continue;
                            }

                            let battle = match &mut battle_state.battle {
                                None => {
                                    log::info!("no battle in progress, dropping input");
                                    continue;
                                },
                                Some(b) => b,
                            };

                            battle.add_input(battle.remote_player_index(), input::Input{
                                local_tick: input.local_tick,
                                remote_tick: input.remote_tick,
                                joyflags: input.joyflags as u16,
                                custom_screen_state: input.custom_screen_state as u8,
                                turn: if input.turn.is_empty() { None } else { Some(input.turn.as_slice().try_into().unwrap()) },
                            }).await;
                        },
                        p => anyhow::bail!("unknown packet: {:?}", p)
                    }
                }
                Ok(())
            } => {
                r
            }
        }
    }

    pub fn cancel(&self) {
        self.cancellation_token.cancel();
    }

    pub async fn poll_for_ready(&self) -> anyhow::Result<bool> {
        match &*self.negotiation.lock().await {
            Negotiation::Negotiated { .. } => Ok(true),
            Negotiation::NotReady => Ok(false),
            Negotiation::Err(e) => anyhow::bail!("{}", e),
        }
    }

    pub async fn send_init(
        &self,
        battle_number: u32,
        input_delay: u32,
        marshaled: &[u8; 0x100],
    ) -> anyhow::Result<()> {
        let dc = match &*self.negotiation.lock().await {
            Negotiation::Negotiated { dc, .. } => dc.clone(),
            Negotiation::NotReady => anyhow::bail!("not ready"),
            Negotiation::Err(e) => anyhow::bail!("{}", e),
        };
        dc.send(
            protocol::Packet {
                which: Some(protocol::packet::Which::Init(protocol::Init {
                    battle_number,
                    input_delay,
                    marshaled: marshaled.to_vec(),
                })),
            }
            .encode_to_vec()
            .as_slice(),
        )
        .await?;
        Ok(())
    }

    pub async fn send_input(
        &self,
        battle_number: u32,
        local_tick: u32,
        remote_tick: u32,
        joyflags: u16,
        custom_screen_state: u8,
        turn: &Option<[u8; 0x100]>,
    ) -> anyhow::Result<()> {
        let dc = match &*self.negotiation.lock().await {
            Negotiation::Negotiated { dc, .. } => dc.clone(),
            Negotiation::NotReady => anyhow::bail!("not ready"),
            Negotiation::Err(e) => anyhow::bail!("{}", e),
        };
        dc.send(
            protocol::Packet {
                which: Some(protocol::packet::Which::Input(protocol::Input {
                    battle_number,
                    local_tick,
                    remote_tick,
                    joyflags: joyflags as u32,
                    custom_screen_state: custom_screen_state as u32,
                    turn: if let Some(turn) = turn {
                        turn.to_vec()
                    } else {
                        vec![]
                    },
                })),
            }
            .encode_to_vec()
            .as_slice(),
        )
        .await?;
        Ok(())
    }

    pub async fn set_won_last_battle(&self, won: bool) {
        self.battle_state.lock().await.won_last_battle = won;
    }

    pub async fn rng(
        &self,
    ) -> anyhow::Result<tokio::sync::MappedMutexGuard<'_, rand_pcg::Mcg128Xsl64>> {
        let negotiation = self.negotiation.lock().await;
        match &*negotiation {
            Negotiation::Negotiated { .. } => {
                Ok(tokio::sync::MutexGuard::map(negotiation, |n| match n {
                    Negotiation::Negotiated { rng, .. } => rng,
                    _ => unreachable!(),
                }))
            }
            Negotiation::NotReady => anyhow::bail!("not ready"),
            Negotiation::Err(e) => anyhow::bail!("{}", e),
        }
    }

    pub fn match_type(&self) -> u16 {
        self.match_type
    }

    // TODO: read remote init
    // TODO: handle conn
    // TODO: end battle
}

struct LocalPendingTurn {
    marshaled: [u8; 0x100],
    ticks_left: u8,
}

pub struct Battle {
    is_p2: bool,
    iq: input::Queue,
    remote_delay: u32,
    is_accepting_input: bool,
    is_over: bool,
    last_committed_remote_input: input::Input,
    last_input: Option<[input::Input; 2]>,
    state_committed: tokio::sync::Notify,
    committed_state: Option<mgba::state::State>,
    local_pending_turn: Option<LocalPendingTurn>,
}

impl Battle {
    pub fn local_player_index(&self) -> u8 {
        if self.is_p2 {
            1
        } else {
            0
        }
    }

    pub fn remote_player_index(&self) -> u8 {
        1 - self.local_player_index()
    }

    pub fn set_committed_state(&mut self, state: mgba::state::State) {
        self.committed_state = Some(state);
        self.state_committed.notify_one();
    }

    pub fn set_last_input(&mut self, inp: [input::Input; 2]) {
        self.last_input = Some(inp);
    }

    pub fn take_last_input(&mut self) -> Option<[input::Input; 2]> {
        self.last_input.take()
    }

    pub fn is_p2(&self) -> bool {
        self.is_p2
    }

    pub fn local_delay(&self) -> u32 {
        self.iq.local_delay()
    }

    pub fn set_remote_delay(&mut self, delay: u32) {
        self.remote_delay = delay;
    }

    pub fn remote_delay(&self) -> u32 {
        self.remote_delay
    }

    pub fn start_accepting_input(&mut self) {
        self.is_accepting_input = true;
    }

    pub fn is_accepting_input(&self) -> bool {
        self.is_accepting_input
    }

    pub fn mark_over(&mut self) {
        self.is_over = true;
    }

    pub fn is_over(&self) -> bool {
        self.is_over
    }

    pub fn last_committed_remote_input(&self) -> input::Input {
        self.last_committed_remote_input.clone()
    }

    pub fn committed_state(&self) -> &Option<mgba::state::State> {
        &self.committed_state
    }

    pub async fn consume_and_peek_local(&mut self) -> (Vec<[input::Input; 2]>, Vec<input::Input>) {
        let (input_pairs, left) = self.iq.consume_and_peek_local().await;
        if let Some(last) = input_pairs.last() {
            self.last_committed_remote_input = last[1 - self.local_player_index() as usize].clone();
        }
        (input_pairs, left)
    }

    pub async fn add_input(&mut self, player_index: u8, input: input::Input) {
        self.iq.add_input(player_index, input).await;
    }

    pub fn add_local_pending_turn(&mut self, marshaled: [u8; 0x100]) {
        self.local_pending_turn = Some(LocalPendingTurn {
            ticks_left: 64,
            marshaled,
        })
    }

    pub fn take_local_pending_turn(&mut self) -> Option<[u8; 0x100]> {
        match &mut self.local_pending_turn {
            Some(lpt) => {
                if lpt.ticks_left > 0 {
                    lpt.ticks_left -= 1;
                    if lpt.ticks_left == 0 {
                        let t = lpt.marshaled;
                        self.local_pending_turn = None;
                        Some(t)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            None => None,
        }
    }
}
