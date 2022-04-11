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

pub struct BattleState {
    pub number: u32,
    pub battle: Option<Battle>,
    pub won_last_battle: bool,
}

enum Negotiation {
    NotReady,
    Negotiated {
        peer_conn: webrtc::peer_connection::RTCPeerConnection,
        dc: std::sync::Arc<datachannel::DataChannel>,
        rng: rand_pcg::Mcg128Xsl64,
    },
    Err(NegotiationError),
}

pub struct Match {
    cancellation_token: tokio_util::sync::CancellationToken,
    r#impl: std::sync::Arc<MatchImpl>,
}

pub struct Settings {
    pub matchmaking_connect_addr: String,
    pub webrtc_config: webrtc::peer_connection::configuration::RTCConfiguration,
}

struct MatchImpl {
    negotiation: tokio::sync::Mutex<Negotiation>,
    session_id: String,
    match_type: u16,
    game_title: String,
    game_crc32: u32,
    input_delay: u32,
    settings: Settings,
    battle_state: tokio::sync::Mutex<BattleState>,
    remote_init_sender: tokio::sync::mpsc::Sender<protocol::Init>,
    remote_init_receiver: tokio::sync::Mutex<tokio::sync::mpsc::Receiver<protocol::Init>>,
}

#[derive(Debug)]
pub enum NegotiationError {
    ExpectedHello,
    ExpectedHola,
    IdenticalCommitment,
    ProtocolVersionMismatch,
    MatchTypeMismatch,
    GameMismatch,
    InvalidCommitment,
    WebRTC(webrtc::Error),
    IO(std::io::Error),
    Signor(signor::Error),
}

impl std::fmt::Display for NegotiationError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            NegotiationError::ExpectedHello => write!(f, "expected hello"),
            NegotiationError::ExpectedHola => write!(f, "expected hola"),
            NegotiationError::IdenticalCommitment => write!(f, "identical commitment"),
            NegotiationError::ProtocolVersionMismatch => write!(f, "protocol version mismatch"),
            NegotiationError::MatchTypeMismatch => write!(f, "match type mismatch"),
            NegotiationError::GameMismatch => write!(f, "game mismatch"),
            NegotiationError::InvalidCommitment => write!(f, "invalid commitment"),
            NegotiationError::WebRTC(e) => write!(f, "WebRTC error: {}", e),
            NegotiationError::IO(e) => write!(f, "IO error: {}", e),
            NegotiationError::Signor(e) => write!(f, "signor error: {}", e),
        }
    }
}

impl std::error::Error for NegotiationError {}

pub enum NegotiationStatus {
    Ready,
    NotReady,
    MatchTypeMismatch,
    GameMismatch,
    Failed(anyhow::Error),
}

impl From<webrtc::Error> for NegotiationError {
    fn from(e: webrtc::Error) -> Self {
        NegotiationError::WebRTC(e)
    }
}

impl From<std::io::Error> for NegotiationError {
    fn from(e: std::io::Error) -> Self {
        NegotiationError::IO(e)
    }
}

impl From<signor::Error> for NegotiationError {
    fn from(e: signor::Error) -> Self {
        NegotiationError::Signor(e)
    }
}

impl MatchImpl {
    async fn negotiate(&self) -> Result<(), NegotiationError> {
        log::info!("negotiating match, session_id = {}", self.session_id);

        let mut sc = signor::Client::new(&self.settings.matchmaking_connect_addr).await?;

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

        log::info!(
            "local sdp: {}",
            peer_conn.local_description().await.expect("local sdp").sdp
        );
        log::info!(
            "remote sdp: {}",
            peer_conn
                .remote_description()
                .await
                .expect("remote sdp")
                .sdp
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
                None => {
                    return Err(NegotiationError::ExpectedHello);
                }
            }
            .as_slice(),
        )
        .map_err(|_| NegotiationError::ExpectedHello)?
        {
            protocol::Packet {
                which: Some(protocol::packet::Which::Hello(hello)),
            } => hello,
            _ => {
                return Err(NegotiationError::ExpectedHello);
            }
        };

        log::info!("their hello={:?}", hello);

        if commitment.ct_eq(hello.rng_commitment.as_slice()).into() {
            return Err(NegotiationError::IdenticalCommitment);
        }

        if hello.protocol_version != protocol::VERSION {
            return Err(NegotiationError::ProtocolVersionMismatch);
        }

        if hello.match_type != self.match_type as u32 {
            return Err(NegotiationError::MatchTypeMismatch);
        }

        if hello.game_title[..8] != self.game_title[..8] {
            return Err(NegotiationError::GameMismatch);
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
                None => {
                    return Err(NegotiationError::ExpectedHola);
                }
            }
            .as_slice(),
        )
        .map_err(|_| NegotiationError::ExpectedHola)?
        {
            protocol::Packet {
                which: Some(protocol::packet::Which::Hola(hola)),
            } => hola,
            _ => {
                return Err(NegotiationError::ExpectedHola);
            }
        };

        log::info!("their hola={:?}", hola);

        if !bool::from(make_rng_commitment(&hola.rng_nonce)?.ct_eq(hello.rng_commitment.as_slice()))
        {
            return Err(NegotiationError::InvalidCommitment);
        }

        log::info!("connection ok!");

        let seed = hola
            .rng_nonce
            .iter()
            .zip(nonce.iter())
            .map(|(&x1, &x2)| x1 ^ x2)
            .collect::<Vec<u8>>();

        let mut rng = rand_pcg::Mcg128Xsl64::from_seed(seed.try_into().expect("rng seed"));

        self.battle_state.lock().await.won_last_battle =
            rng.gen::<bool>() == (side == signor::ConnectionSide::Polite);
        *self.negotiation.lock().await = Negotiation::Negotiated { dc, peer_conn, rng };
        Ok(())
    }

    async fn run(&self) -> anyhow::Result<()> {
        if let Err(e) = self.negotiate().await {
            let e2 = anyhow::format_err!("{}", e);
            *self.negotiation.lock().await = Negotiation::Err(e);
            return Err(e2);
        }

        let dc = match &*self.negotiation.lock().await {
            Negotiation::Negotiated { dc, .. } => dc.clone(),
            _ => unreachable!(),
        };

        loop {
            match match protocol::Packet::decode(
                match dc.receive().await {
                    None => break,
                    Some(buf) => buf,
                }
                .as_slice(),
            )?
            .which
            {
                None => break,
                Some(b) => b,
            } {
                protocol::packet::Which::Init(init) => {
                    self.remote_init_sender
                        .send(init)
                        .await
                        .expect("receive init");
                }
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
                        }
                        Some(b) => b,
                    };

                    battle
                        .add_remote_input(input::Input {
                            local_tick: input.local_tick,
                            remote_tick: input.remote_tick,
                            joyflags: input.joyflags as u16,
                            custom_screen_state: input.custom_screen_state as u8,
                            turn: input.turn,
                        })
                        .await;
                }
                p => anyhow::bail!("unknown packet: {:?}", p),
            }
        }

        Ok(())
    }
}

fn make_rng_commitment(nonce: &[u8]) -> std::io::Result<[u8; 32]> {
    let mut shake128 = sha3::Shake128::default();
    shake128.write_all(b"syncrand:nonce:")?;
    shake128.write_all(&nonce)?;

    let mut commitment = [0u8; 32];
    shake128
        .finalize_xof()
        .read_exact(commitment.as_mut_slice())?;

    Ok(commitment)
}

impl Drop for Match {
    fn drop(&mut self) {
        self.cancellation_token.cancel();
    }
}

impl Match {
    pub fn new(
        session_id: String,
        match_type: u16,
        game_title: String,
        game_crc32: u32,
        input_delay: u32,
        settings: Settings,
    ) -> Self {
        let (remote_init_sender, remote_init_receiver) = tokio::sync::mpsc::channel(1);
        let r#impl = std::sync::Arc::new(MatchImpl {
            negotiation: tokio::sync::Mutex::new(Negotiation::NotReady),
            session_id,
            match_type,
            game_title,
            game_crc32,
            input_delay,
            settings,
            battle_state: tokio::sync::Mutex::new(BattleState {
                number: 0,
                battle: None,
                won_last_battle: false,
            }),
            remote_init_sender,
            remote_init_receiver: tokio::sync::Mutex::new(remote_init_receiver),
        });
        Match {
            cancellation_token: tokio_util::sync::CancellationToken::new(),
            r#impl,
        }
    }

    pub async fn lock_battle_state(&self) -> tokio::sync::MutexGuard<'_, BattleState> {
        self.r#impl.battle_state.lock().await
    }

    pub async fn receive_remote_init(&self) -> Option<protocol::Init> {
        self.r#impl.remote_init_receiver.lock().await.recv().await
    }

    pub async fn poll_for_ready(&self) -> NegotiationStatus {
        match &*self.r#impl.negotiation.lock().await {
            Negotiation::Negotiated { .. } => NegotiationStatus::Ready,
            Negotiation::NotReady => NegotiationStatus::NotReady,
            Negotiation::Err(NegotiationError::GameMismatch) => NegotiationStatus::GameMismatch,
            Negotiation::Err(NegotiationError::MatchTypeMismatch) => {
                NegotiationStatus::MatchTypeMismatch
            }
            Negotiation::Err(e) => NegotiationStatus::Failed(anyhow::format_err!("{}", e)),
        }
    }

    pub async fn send_init(
        &self,
        battle_number: u32,
        input_delay: u32,
        marshaled: &[u8],
    ) -> anyhow::Result<()> {
        let dc = match &*self.r#impl.negotiation.lock().await {
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
        turn: Vec<u8>,
    ) -> anyhow::Result<()> {
        let dc = match &*self.r#impl.negotiation.lock().await {
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
                    turn,
                })),
            }
            .encode_to_vec()
            .as_slice(),
        )
        .await?;
        Ok(())
    }

    pub async fn rng(
        &self,
    ) -> anyhow::Result<tokio::sync::MappedMutexGuard<'_, rand_pcg::Mcg128Xsl64>> {
        let negotiation = self.r#impl.negotiation.lock().await;
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
        self.r#impl.match_type
    }

    pub async fn start_battle(&self) {
        let mut battle_state = self.r#impl.battle_state.lock().await;
        let local_player_index = if battle_state.won_last_battle { 0 } else { 1 };
        log::info!(
            "starting battle: local_player_index = {}",
            local_player_index
        );
        battle_state.battle = Some(Battle {
            local_player_index,
            iq: input::PairQueue::new(60, self.r#impl.input_delay),
            remote_delay: 0,
            is_accepting_input: false,
            last_committed_remote_input: input::Input {
                local_tick: 0,
                remote_tick: 0,
                joyflags: 0xfc00,
                custom_screen_state: 0,
                turn: vec![],
            },
            last_input: None,
            state_committed_notify: tokio::sync::Notify::new(),
            committed_state: None,
            local_pending_turn: None,
            local_joyflags: 0xfc00,
        });
    }

    pub async fn end_battle(&self) {
        self.r#impl.battle_state.lock().await.battle = None;
    }

    pub fn start(&self, handle: tokio::runtime::Handle) {
        let cancellation_token = self.cancellation_token.clone();
        let r#impl = self.r#impl.clone();
        handle.spawn(async move {
            tokio::select! {
                _ = cancellation_token.cancelled() => {},
                _ = r#impl.run() => {},
            };
            if let Negotiation::Negotiated { dc, peer_conn, .. } = &*r#impl.negotiation.lock().await
            {
                let _ = dc.close().await;
                let _ = peer_conn.close().await;
            }
        });
    }
}

struct LocalPendingTurn {
    marshaled: Vec<u8>,
    ticks_left: u8,
}

pub struct Battle {
    local_player_index: u8,
    iq: input::PairQueue<input::Input>,
    remote_delay: u32,
    is_accepting_input: bool,
    last_committed_remote_input: input::Input,
    last_input: Option<input::Pair<input::Input>>,
    state_committed_notify: tokio::sync::Notify,
    committed_state: Option<mgba::state::State>,
    local_pending_turn: Option<LocalPendingTurn>,
    local_joyflags: u16,
}

impl Battle {
    pub fn local_player_index(&self) -> u8 {
        self.local_player_index
    }

    pub fn remote_player_index(&self) -> u8 {
        1 - self.local_player_index
    }

    pub fn set_committed_state(&mut self, state: mgba::state::State) {
        self.committed_state = Some(state);
        self.state_committed_notify.notify_one();
    }

    pub fn set_last_input(&mut self, inp: input::Pair<input::Input>) {
        self.last_input = Some(inp);
    }

    pub fn take_last_input(&mut self) -> Option<input::Pair<input::Input>> {
        self.last_input.take()
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

    pub async fn local_queue_length(&self) -> usize {
        self.iq.local_queue_length().await
    }

    pub async fn remote_queue_length(&self) -> usize {
        self.iq.remote_queue_length().await
    }

    pub fn start_accepting_input(&mut self) {
        self.is_accepting_input = true;
    }

    pub fn is_accepting_input(&self) -> bool {
        self.is_accepting_input
    }

    pub fn last_committed_remote_input(&self) -> input::Input {
        self.last_committed_remote_input.clone()
    }

    pub fn committed_state(&self) -> &Option<mgba::state::State> {
        &self.committed_state
    }

    pub async fn consume_and_peek_local(
        &mut self,
    ) -> (Vec<input::Pair<input::Input>>, Vec<input::Input>) {
        let (input_pairs, left) = self.iq.consume_and_peek_local().await;
        if let Some(last) = input_pairs.last() {
            self.last_committed_remote_input = last.remote.clone();
        }
        (input_pairs, left)
    }

    pub async fn add_local_input(&mut self, input: input::Input) {
        self.iq.add_local_input(input).await;
    }

    pub async fn add_remote_input(&mut self, input: input::Input) {
        self.iq.add_remote_input(input).await;
    }

    pub fn add_local_pending_turn(&mut self, marshaled: Vec<u8>) {
        self.local_pending_turn = Some(LocalPendingTurn {
            ticks_left: 64,
            marshaled,
        })
    }

    pub fn take_local_pending_turn(&mut self) -> Vec<u8> {
        match &mut self.local_pending_turn {
            Some(lpt) => {
                if lpt.ticks_left > 0 {
                    lpt.ticks_left -= 1;
                    if lpt.ticks_left != 0 {
                        return vec![];
                    }
                    self.local_pending_turn.take().unwrap().marshaled
                } else {
                    vec![]
                }
            }
            None => vec![],
        }
    }

    pub fn set_local_joyflags(&mut self, joyflags: u16) {
        self.local_joyflags = joyflags;
    }

    pub fn local_joyflags(&self) -> u16 {
        self.local_joyflags
    }
}
