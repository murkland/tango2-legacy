use crate::audio;
use crate::compat;
use crate::datachannel;
use crate::facade;
use crate::fastforwarder;
use crate::hooks;
use crate::input;
use crate::loaded;
use crate::protocol;
use crate::replay;
use crate::transport;
use rand::Rng;
use rand::SeedableRng;
use sha3::digest::ExtendableOutput;
use std::io::Read;
use std::io::Write;
use subtle::ConstantTimeEq;

pub struct BattleState {
    pub number: u8,
    pub battle: Option<Battle>,
    pub won_last_battle: bool,
}

enum Negotiation {
    NotReady(NegotiationProgress),
    Negotiated {
        peer_conn: webrtc::peer_connection::RTCPeerConnection,
        dc: std::sync::Arc<datachannel::DataChannel>,
        rng: rand_pcg::Mcg128Xsl64,
    },
    Err(NegotiationError),
}

pub struct Match {
    cancellation_token: tokio_util::sync::CancellationToken,
    in_progress: std::sync::Arc<tokio::sync::Mutex<Option<std::sync::Arc<InProgress>>>>,
}

pub struct Settings {
    pub matchmaking_connect_addr: String,
    pub make_webrtc_config:
        Box<dyn Fn() -> webrtc::peer_connection::configuration::RTCConfiguration + Send + Sync>,
}

pub struct InProgress {
    compat_list: compat::CompatList,
    audio_supported_config: cpal::SupportedStreamConfig,
    rom_path: std::path::PathBuf,
    hooks: &'static Box<dyn hooks::Hooks + Send + Sync>,
    negotiation: tokio::sync::Mutex<Negotiation>,
    start_time: std::time::SystemTime,
    session_id: String,
    match_type: u16,
    game_title: String,
    game_crc32: u32,
    input_delay: u32,
    settings: Settings,
    battle_state: tokio::sync::Mutex<BattleState>,
    remote_init_sender: tokio::sync::mpsc::Sender<protocol::Init>,
    remote_init_receiver: tokio::sync::Mutex<tokio::sync::mpsc::Receiver<protocol::Init>>,
    audio_mux: audio::mux_stream::MuxStream,
    audio_save_state_holder: std::sync::Arc<parking_lot::Mutex<Option<mgba::state::State>>>,
}

#[derive(Debug)]
pub enum NegotiationError {
    ExpectedHello,
    ExpectedHola,
    IdenticalCommitment,
    ProtocolVersionMismatch,
    MatchTypeMismatch,
    IncompatibleGames,
    InvalidCommitment,
    Other(anyhow::Error),
}

impl From<anyhow::Error> for NegotiationError {
    fn from(err: anyhow::Error) -> Self {
        NegotiationError::Other(err)
    }
}

impl From<webrtc::Error> for NegotiationError {
    fn from(err: webrtc::Error) -> Self {
        NegotiationError::Other(err.into())
    }
}

impl From<std::io::Error> for NegotiationError {
    fn from(err: std::io::Error) -> Self {
        NegotiationError::Other(err.into())
    }
}

impl std::fmt::Display for NegotiationError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            NegotiationError::ExpectedHello => write!(f, "expected hello"),
            NegotiationError::ExpectedHola => write!(f, "expected hola"),
            NegotiationError::IdenticalCommitment => write!(f, "identical commitment"),
            NegotiationError::ProtocolVersionMismatch => write!(f, "protocol version mismatch"),
            NegotiationError::MatchTypeMismatch => write!(f, "match type mismatch"),
            NegotiationError::IncompatibleGames => write!(f, "game mismatch"),
            NegotiationError::InvalidCommitment => write!(f, "invalid commitment"),
            NegotiationError::Other(e) => write!(f, "other error: {}", e),
        }
    }
}

impl std::error::Error for NegotiationError {}

pub enum NegotiationFailure {
    ProtocolVersionMismatch,
    MatchTypeMismatch,
    IncompatibleGames,
    Unknown,
}

pub enum NegotiationStatus {
    Ready,
    NotReady(NegotiationProgress),
    Failed(NegotiationFailure),
}

#[derive(Clone, Debug)]
pub enum NegotiationProgress {
    NotStarted,
    Signalling,
    Handshaking,
}

impl InProgress {
    async fn negotiate(&self) -> Result<(), NegotiationError> {
        log::info!("negotiating match, session_id = {}", self.session_id);

        *self.negotiation.lock().await = Negotiation::NotReady(NegotiationProgress::Signalling);
        let api = webrtc::api::APIBuilder::new().build();
        let (peer_conn, dc, side) = tango_matchmaking::client::connect(
            &self.settings.matchmaking_connect_addr,
            || async {
                let peer_conn = api
                    .new_peer_connection((self.settings.make_webrtc_config)())
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

        *self.negotiation.lock().await = Negotiation::NotReady(NegotiationProgress::Handshaking);
        dc.send(
            protocol::Packet::Hello(protocol::Hello {
                protocol_version: protocol::VERSION,
                game_title: self.game_title.clone(),
                game_crc32: self.game_crc32,
                match_type: self.match_type,
                rng_commitment: commitment.to_vec(),
            })
            .serialize()
            .expect("serialize")
            .as_slice(),
        )
        .await?;

        let hello = match protocol::Packet::deserialize(
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
            protocol::Packet::Hello(hello) => hello,
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

        if hello.match_type != self.match_type {
            return Err(NegotiationError::MatchTypeMismatch);
        }

        let my_game_id = self
            .compat_list
            .id_by_title_and_crc32(&self.game_title, self.game_crc32)
            .unwrap();

        let their_game_id = if let Some(id) = self
            .compat_list
            .id_by_title_and_crc32(&hello.game_title, hello.game_crc32)
        {
            id
        } else {
            return Err(NegotiationError::IncompatibleGames);
        };

        // THIS IS NOT AN ANTI-CHEAT MECHANISM. THIS IS NOT AN ANTI-CHEAT MECHANISM. THIS IS NOT AN ANTI-CHEAT MECHANISM.
        if !self.compat_list.is_compatible(my_game_id, their_game_id) {
            return Err(NegotiationError::IncompatibleGames);
        }

        dc.send(
            protocol::Packet::Hola(protocol::Hola {
                rng_nonce: nonce.to_vec(),
            })
            .serialize()
            .expect("serialize")
            .as_slice(),
        )
        .await?;

        let hola = match protocol::Packet::deserialize(
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
            protocol::Packet::Hola(hola) => hola,
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
            rng.gen::<bool>() == (side == tango_matchmaking::client::ConnectionSide::Polite);
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
            match protocol::Packet::deserialize(
                match dc.receive().await {
                    None => break,
                    Some(buf) => buf,
                }
                .as_slice(),
            )? {
                protocol::Packet::Init(init) => {
                    self.remote_init_sender
                        .send(init)
                        .await
                        .expect("receive init");
                }
                protocol::Packet::Input(input) => {
                    let state_committed_rx = {
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
                        battle.state_committed_rx.take()
                    };

                    if let Some(state_committed_rx) = state_committed_rx {
                        state_committed_rx.await.unwrap();
                    }

                    let mut battle_state = self.battle_state.lock().await;

                    let battle = match &mut battle_state.battle {
                        None => {
                            log::info!("no battle in progress, dropping input");
                            continue;
                        }
                        Some(b) => b,
                    };

                    if !battle.add_remote_input(input::Input {
                        local_tick: input.local_tick,
                        remote_tick: input.remote_tick,
                        joyflags: input.joyflags as u16,
                        custom_screen_state: input.custom_screen_state as u8,
                        turn: input.turn,
                    }) {
                        anyhow::bail!("remote overflowed our input buffer");
                    }
                }
                p => anyhow::bail!("unknown packet: {:?}", p),
            }
        }

        Ok(())
    }

    pub async fn lock_battle_state(&self) -> tokio::sync::MutexGuard<'_, BattleState> {
        self.battle_state.lock().await
    }

    pub async fn receive_remote_init(&self) -> Option<protocol::Init> {
        let mut remote_init_receiver = self.remote_init_receiver.lock().await;
        remote_init_receiver.recv().await
    }

    async fn poll_for_ready(&self) -> NegotiationStatus {
        let negotiation = self.negotiation.lock().await;
        match &*negotiation {
            Negotiation::Negotiated { .. } => NegotiationStatus::Ready,
            Negotiation::NotReady(p) => NegotiationStatus::NotReady(p.clone()),
            Negotiation::Err(NegotiationError::IncompatibleGames) => {
                NegotiationStatus::Failed(NegotiationFailure::IncompatibleGames)
            }
            Negotiation::Err(NegotiationError::MatchTypeMismatch) => {
                NegotiationStatus::Failed(NegotiationFailure::MatchTypeMismatch)
            }
            Negotiation::Err(NegotiationError::ProtocolVersionMismatch) => {
                NegotiationStatus::Failed(NegotiationFailure::ProtocolVersionMismatch)
            }
            Negotiation::Err(_) => NegotiationStatus::Failed(NegotiationFailure::Unknown),
        }
    }

    pub async fn transport(&self) -> anyhow::Result<transport::Transport> {
        let negotiation = self.negotiation.lock().await;
        let dc = match &*negotiation {
            Negotiation::Negotiated { dc, .. } => dc.clone(),
            Negotiation::NotReady(_) => anyhow::bail!("not ready"),
            Negotiation::Err(e) => anyhow::bail!("{}", e),
        };
        Ok(transport::Transport::new(dc))
    }

    pub async fn lock_rng(
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
            Negotiation::NotReady(_) => anyhow::bail!("not ready"),
            Negotiation::Err(e) => anyhow::bail!("{}", e),
        }
    }

    pub fn match_type(&self) -> u16 {
        self.match_type
    }

    pub async fn start_battle(&self, core: mgba::core::CoreMutRef<'_>) -> anyhow::Result<()> {
        let mut battle_state = self.battle_state.lock().await;
        battle_state.number += 1;
        let local_player_index = if battle_state.won_last_battle { 0 } else { 1 };
        log::info!(
            "starting battle: local_player_index = {}",
            local_player_index
        );
        let replay_filename = format!(
            "{}_battle{}_p{}.tangoreplay",
            time::OffsetDateTime::from(self.start_time)
                .format(time::macros::format_description!(
                    "[year padding:zero][month padding:zero repr:numerical][day padding:zero][hour padding:zero][minute padding:zero][second padding:zero]"
                ))?,
            battle_state.number,
            local_player_index + 1
        );
        let replay_file =
            std::fs::File::create(std::path::Path::new("replays").join(&replay_filename))?;
        log::info!("opened replay: {}", replay_filename);

        let mut audio_core = mgba::core::Core::new_gba("tango")?;
        let rom_vf = mgba::vfile::VFile::open(&self.rom_path, mgba::vfile::flags::O_RDONLY)?;
        audio_core.as_mut().load_rom(rom_vf)?;
        audio_core.as_mut().reset();
        audio_core.set_traps(self.hooks.audio_traps(facade::AudioFacade::new(
            self.audio_save_state_holder.clone(),
            local_player_index,
        )));

        let audio_core_thread = mgba::thread::Thread::new(audio_core);
        audio_core_thread.start();

        let audio_core_handle = audio_core_thread.handle();
        audio_core_handle.pause();
        let audio_core_mux_handle = self.audio_mux.open_stream();
        audio_core_mux_handle.set_stream(audio::timewarp_stream::TimewarpStream::new(
            audio_core_handle.clone(),
            self.audio_supported_config.sample_rate(),
            self.audio_supported_config.channels(),
        ));
        {
            let audio_core_mux_handle = audio_core_mux_handle.clone();
            let save_state = core.save_state().expect("save state");
            audio_core_handle.run_on_core(move |mut core| {
                core.gba_mut()
                    .sync_mut()
                    .as_mut()
                    .expect("sync")
                    .set_fps_target(loaded::EXPECTED_FPS as f32);
                core.load_state(&save_state).expect("load state");
                audio_core_mux_handle.switch();
            });
        }
        audio_core_handle.unpause();

        let (tx, rx) = tokio::sync::oneshot::channel();
        battle_state.battle = Some(Battle {
            local_player_index,
            iq: input::PairQueue::new(60, self.input_delay),
            remote_delay: 0,
            is_accepting_input: false,
            last_committed_remote_input: input::Input {
                local_tick: 0,
                remote_tick: 0,
                joyflags: 0,
                custom_screen_state: 0,
                turn: vec![],
            },
            last_input: None,
            state_committed_tx: Some(tx),
            state_committed_rx: Some(rx),
            committed_state: None,
            local_pending_turn: None,
            replay_writer: replay::Writer::new(Box::new(replay_file), local_player_index)?,
            fastforwarder: fastforwarder::Fastforwarder::new(
                &self.rom_path,
                self.hooks,
                local_player_index,
            )?,
            audio_save_state_holder: self.audio_save_state_holder.clone(),
            _audio_core_thread: audio_core_thread,
            _audio_core_mux_handle: audio_core_mux_handle,
        });
        Ok(())
    }

    pub async fn end_battle(&self) {
        self.battle_state.lock().await.battle = None;
    }
}

fn make_rng_commitment(nonce: &[u8]) -> std::io::Result<[u8; 32]> {
    let mut shake128 = sha3::Shake128::default();
    shake128.write_all(b"syncrand:nonce:")?;
    shake128.write_all(nonce)?;

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
        compat_list: compat::CompatList,
        audio_supported_config: cpal::SupportedStreamConfig,
        rom_path: std::path::PathBuf,
        hooks: &'static Box<dyn hooks::Hooks + Send + Sync>,
        audio_mux: audio::mux_stream::MuxStream,
        audio_save_state_holder: std::sync::Arc<parking_lot::Mutex<Option<mgba::state::State>>>,
        session_id: String,
        match_type: u16,
        game_title: String,
        game_crc32: u32,
        input_delay: u32,
        settings: Settings,
    ) -> Self {
        let (remote_init_sender, remote_init_receiver) = tokio::sync::mpsc::channel(1);
        Match {
            cancellation_token: tokio_util::sync::CancellationToken::new(),
            in_progress: std::sync::Arc::new(tokio::sync::Mutex::new(Some(std::sync::Arc::new(
                InProgress {
                    compat_list,
                    audio_supported_config,
                    rom_path,
                    hooks,
                    negotiation: tokio::sync::Mutex::new(Negotiation::NotReady(
                        NegotiationProgress::NotStarted,
                    )),
                    start_time: std::time::SystemTime::now(),
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
                    audio_mux,
                    audio_save_state_holder,
                },
            )))),
        }
    }

    pub async fn lock_in_progress(
        &self,
    ) -> tokio::sync::MutexGuard<'_, Option<std::sync::Arc<InProgress>>> {
        self.in_progress.lock().await
    }

    pub async fn poll_for_ready(&self) -> NegotiationStatus {
        let in_progress = self.lock_in_progress().await.clone();
        let in_progress = if let Some(in_progress) = in_progress.as_ref() {
            in_progress
        } else {
            return NegotiationStatus::Failed(NegotiationFailure::Unknown);
        };
        in_progress.poll_for_ready().await
    }

    pub async fn start(&self, handle: tokio::runtime::Handle) {
        let cancellation_token = self.cancellation_token.clone();
        let in_progress = self.in_progress.clone();
        handle.spawn(async move {
            let inner_in_progress = in_progress.lock().await.clone().unwrap();
            tokio::select! {
                _ = cancellation_token.cancelled() => {},
                Err(e) = inner_in_progress.run() => {
                    log::info!("match thread ending: {:?}", e);
                },
            };
            if let Negotiation::Negotiated { dc, peer_conn, .. } =
                &*inner_in_progress.negotiation.lock().await
            {
                let _ = dc.close().await;
                let _ = peer_conn.close().await;
            }

            *in_progress.lock().await = None;
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
    state_committed_tx: Option<tokio::sync::oneshot::Sender<()>>,
    state_committed_rx: Option<tokio::sync::oneshot::Receiver<()>>,
    committed_state: Option<mgba::state::State>,
    local_pending_turn: Option<LocalPendingTurn>,
    replay_writer: replay::Writer,
    fastforwarder: fastforwarder::Fastforwarder,
    audio_save_state_holder: std::sync::Arc<parking_lot::Mutex<Option<mgba::state::State>>>,
    _audio_core_thread: mgba::thread::Thread,
    _audio_core_mux_handle: audio::mux_stream::MuxHandle,
}

impl Battle {
    pub fn fastforwarder(&mut self) -> &mut fastforwarder::Fastforwarder {
        &mut self.fastforwarder
    }

    pub fn replay_writer(&mut self) -> &mut replay::Writer {
        &mut self.replay_writer
    }

    pub fn local_player_index(&self) -> u8 {
        self.local_player_index
    }

    pub fn remote_player_index(&self) -> u8 {
        1 - self.local_player_index
    }

    pub fn set_committed_state(&mut self, state: mgba::state::State) {
        self.committed_state = Some(state);
        if let Some(tx) = self.state_committed_tx.take() {
            let _ = tx.send(());
        }
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

    pub fn local_queue_length(&self) -> usize {
        self.iq.local_queue_length()
    }

    pub fn remote_queue_length(&self) -> usize {
        self.iq.remote_queue_length()
    }

    pub fn mark_accepting_input(&mut self) {
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

    pub fn consume_and_peek_local(
        &mut self,
    ) -> (Vec<input::Pair<input::Input>>, Vec<input::Input>) {
        let (input_pairs, left) = self.iq.consume_and_peek_local();
        if let Some(last) = input_pairs.last() {
            self.last_committed_remote_input = last.remote.clone();
        }
        (input_pairs, left)
    }

    pub fn add_local_input(&mut self, input: input::Input) -> bool {
        log::debug!("local input: {:?}", input);
        self.iq.add_local_input(input)
    }

    pub fn add_remote_input(&mut self, input: input::Input) -> bool {
        log::debug!("remote input: {:?}", input);
        self.iq.add_remote_input(input)
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

    pub fn tps_adjustment(&self) -> i32 {
        let last_local_input = match &self.last_input {
            Some(input::Pair { local, .. }) => local,
            None => {
                return 0;
            }
        };
        (last_local_input.remote_tick as i32
            - last_local_input.local_tick as i32
            - self.local_delay() as i32)
            - (self.last_committed_remote_input.remote_tick as i32
                - self.last_committed_remote_input.local_tick as i32
                - self.remote_delay() as i32)
    }
}

impl Drop for Battle {
    fn drop(&mut self) {
        *self.audio_save_state_holder.lock() = None;
    }
}
