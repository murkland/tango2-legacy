use crate::{battle, bn6, config, fastforwarder, gui, hooks::Hooks, input, tps};
use cpal::traits::StreamTrait;
use parking_lot::Mutex;
use std::sync::Arc;

pub const EXPECTED_FPS: u32 = 60;

pub enum MatchState {
    NoMatch,
    Aborted,
    Match(battle::Match),
}

pub struct Loaded {
    core: Arc<Mutex<mgba::core::Core>>,
    match_state: Arc<tokio::sync::Mutex<MatchState>>,
    joyflags: Arc<std::sync::atomic::AtomicU32>,
    _trapper: mgba::trapper::Trapper,
    _thread: mgba::thread::Thread,
    _stream: cpal::Stream,
}

pub struct BattleStateFacadeGuard<'a> {
    m: &'a battle::Match,
    guard: tokio::sync::MutexGuard<'a, battle::BattleState>,
    fastforwarder: std::sync::Arc<parking_lot::Mutex<fastforwarder::Fastforwarder>>,
}

impl<'a> BattleStateFacadeGuard<'a> {
    pub fn add_local_pending_turn(&mut self, local_turn: Vec<u8>) {
        self.guard
            .battle
            .as_mut()
            .expect("attempted to get battle information while no battle was active!")
            .add_local_pending_turn(local_turn);
    }

    pub fn has_committed_state(&self) -> bool {
        self.guard
            .battle
            .as_ref()
            .expect("attempted to get battle information while no battle was active!")
            .committed_state()
            .is_some()
    }

    pub async fn add_local_input_and_fastforward(
        &mut self,
        current_tick: u32,
        joyflags: u16,
        custom_screen_state: u8,
        turn: Vec<u8>,
    ) -> (
        mgba::state::State,
        mgba::state::State,
        input::Pair<input::Input>,
    ) {
        let fastforwarder = self.fastforwarder.clone();
        let battle_number = self.guard.number;

        let battle = self
            .guard
            .battle
            .as_mut()
            .expect("attempted to get battle information while no battle was active!");

        let local_player_index = battle.local_player_index();
        let local_tick = current_tick + battle.local_delay();
        let remote_tick = battle.last_committed_remote_input().local_tick;

        battle
            .add_local_input(input::Input {
                local_tick,
                remote_tick,
                joyflags,
                custom_screen_state,
                turn: turn.clone(),
            })
            .await;

        self.m
            .transport()
            .await
            .expect("transport not available")
            .send_input(
                battle_number,
                local_tick,
                remote_tick,
                joyflags,
                custom_screen_state,
                turn,
            )
            .await
            .expect("send input");

        let (input_pairs, left) = battle.consume_and_peek_local().await;

        for ip in &input_pairs {
            battle
                .replay_writer()
                .write_input(local_player_index, ip)
                .expect("write input");
        }

        let mut fastforwarder = fastforwarder.lock();
        fastforwarder
            .fastforward(
                battle.committed_state().as_ref().expect("committed state"),
                battle.local_player_index(),
                &input_pairs,
                battle.last_committed_remote_input(),
                &left,
            )
            .expect("fastforward")
    }

    pub fn set_last_input(&mut self, input: input::Pair<input::Input>) {
        let battle = self
            .guard
            .battle
            .as_mut()
            .expect("attempted to get battle information while no battle was active!");
        battle.set_last_input(input)
    }

    pub fn set_committed_state(&mut self, state: mgba::state::State) {
        let battle = self
            .guard
            .battle
            .as_mut()
            .expect("attempted to get battle information while no battle was active!");
        if battle.committed_state().is_none() {
            battle
                .replay_writer()
                .write_state(&state)
                .expect("write state");
        }
        battle.set_committed_state(state);
    }

    pub async fn fill_input_delay(&mut self, current_tick: u32) {
        let battle = self
            .guard
            .battle
            .as_mut()
            .expect("attempted to get battle information while no battle was active!");
        for i in 0..battle.local_delay() {
            battle
                .add_local_input(input::Input {
                    local_tick: current_tick + i,
                    remote_tick: 0,
                    joyflags: 0,
                    custom_screen_state: 0,
                    turn: vec![],
                })
                .await;
        }
        for i in 0..battle.remote_delay() {
            battle
                .add_remote_input(input::Input {
                    local_tick: current_tick + i,
                    remote_tick: 0,
                    joyflags: 0,
                    custom_screen_state: 0,
                    turn: vec![],
                })
                .await;
        }
    }

    pub async fn send_init(&mut self, init: &[u8]) {
        let local_delay = self
            .guard
            .battle
            .as_ref()
            .expect("attempted to get battle information while no battle was active!")
            .local_delay();

        self.m
            .transport()
            .await
            .expect("no transport")
            .send_init(self.guard.number, local_delay, init)
            .await
            .expect("send init");
        log::info!("sent local init: {:?}", init);
    }

    pub async fn receive_init(&mut self) -> Option<Vec<u8>> {
        let init = match self.m.receive_remote_init().await {
            Some(init) => init,
            None => {
                return None;
            }
        };
        log::info!("received remote init: {:?}", init);

        self.guard
            .battle
            .as_mut()
            .expect("attempted to get battle information while no battle was active!")
            .set_remote_delay(init.input_delay);

        Some(init.marshaled)
    }

    pub fn is_active(&self) -> bool {
        self.guard.battle.is_some()
    }

    pub fn mark_accepting_input(&mut self) {
        self.guard
            .battle
            .as_mut()
            .expect("attempted to get battle information while no battle was active!")
            .mark_accepting_input()
    }

    pub fn is_accepting_input(&self) -> bool {
        self.guard
            .battle
            .as_ref()
            .expect("attempted to get battle information while no battle was active!")
            .is_accepting_input()
    }

    pub fn local_player_index(&self) -> u8 {
        self.guard
            .battle
            .as_ref()
            .expect("attempted to get battle information while no battle was active!")
            .local_player_index()
    }

    pub fn remote_player_index(&self) -> u8 {
        self.guard
            .battle
            .as_ref()
            .expect("attempted to get battle information while no battle was active!")
            .remote_player_index()
    }

    pub fn take_last_input(&mut self) -> Option<input::Pair<input::Input>> {
        self.guard
            .battle
            .as_mut()
            .expect("attempted to get battle information while no battle was active!")
            .take_last_input()
    }

    pub fn take_local_pending_turn(&mut self) -> Vec<u8> {
        self.guard
            .battle
            .as_mut()
            .expect("attempted to get battle information while no battle was active!")
            .take_local_pending_turn()
    }

    pub fn set_won_last_battle(&mut self, did_win: bool) {
        self.guard.won_last_battle = did_win;
    }

    pub fn tps_adjustment(&self) -> i32 {
        self.guard
            .battle
            .as_ref()
            .expect("attempted to get battle information while no battle was active!")
            .tps_adjustment()
    }
}

pub struct MatchStateFacadeGuard<'a> {
    guard: tokio::sync::MutexGuard<'a, MatchState>,
    fastforwarder: std::sync::Arc<parking_lot::Mutex<fastforwarder::Fastforwarder>>,
    config: std::sync::Arc<parking_lot::Mutex<config::Config>>,
}

pub enum MatchReadyStatus {
    Ready,
    NotReady,
    Failed,
}

impl<'a> MatchStateFacadeGuard<'a> {
    pub fn is_active(&self) -> bool {
        match &*self.guard {
            MatchState::NoMatch => false,
            MatchState::Aborted => false,
            MatchState::Match(_) => true,
        }
    }

    pub fn is_aborted(&self) -> bool {
        match &*self.guard {
            MatchState::NoMatch => false,
            MatchState::Aborted => true,
            MatchState::Match(_) => false,
        }
    }

    pub async fn poll_for_ready(&self) -> MatchReadyStatus {
        let m = if let MatchState::Match(m) = &*self.guard {
            m
        } else {
            unreachable!();
        };
        match m.poll_for_ready().await {
            battle::NegotiationStatus::Ready => MatchReadyStatus::Ready,
            battle::NegotiationStatus::NotReady(_) => MatchReadyStatus::NotReady,
            battle::NegotiationStatus::MatchTypeMismatch => MatchReadyStatus::Failed,
            battle::NegotiationStatus::GameMismatch => MatchReadyStatus::Failed,
            battle::NegotiationStatus::Failed(_) => MatchReadyStatus::Failed,
        }
    }

    pub fn start(
        &mut self,
        core: mgba::core::CoreMutRef,
        handle: tokio::runtime::Handle,
        match_type: u16,
        s: gui::ConnectRequestState,
        gui_state: std::sync::Arc<gui::State>,
    ) {
        let config = self.config.lock();
        let m = battle::Match::new(
            s.code.to_string(),
            match_type,
            core.as_ref().game_title(),
            core.as_ref().crc32(),
            s.input_delay,
            battle::Settings {
                matchmaking_connect_addr: config.matchmaking.connect_addr.to_string(),
                make_webrtc_config: {
                    let webrtc = config.webrtc.clone();
                    Box::new(move || webrtc.make_webrtc_config())
                },
            },
        );
        m.start(handle);
        *self.guard = MatchState::Match(m);
    }

    pub fn set_match(&mut self, m: battle::Match) {
        *self.guard = MatchState::Match(m);
    }

    pub fn abort(&mut self) {
        *self.guard = MatchState::Aborted;
    }

    pub async fn lock_battle_state(&'a self) -> BattleStateFacadeGuard<'a> {
        let m = if let MatchState::Match(m) = &*self.guard {
            m
        } else {
            unreachable!();
        };
        let guard = m.lock_battle_state().await;
        BattleStateFacadeGuard {
            m,
            guard,
            fastforwarder: self.fastforwarder.clone(),
        }
    }

    pub async fn start_battle(&self) {
        let m = if let MatchState::Match(m) = &*self.guard {
            m
        } else {
            unreachable!();
        };
        m.start_battle().await;
    }

    pub async fn end_battle(&self) {
        let m = if let MatchState::Match(m) = &*self.guard {
            m
        } else {
            unreachable!();
        };
        m.end_battle().await;
    }

    pub async fn lock_rng(&self) -> tokio::sync::MappedMutexGuard<'_, rand_pcg::Mcg128Xsl64> {
        let m = if let MatchState::Match(m) = &*self.guard {
            m
        } else {
            unreachable!();
        };
        m.lock_rng().await.expect("rng")
    }

    pub fn match_type(&self) -> u16 {
        let m = if let MatchState::Match(m) = &*self.guard {
            m
        } else {
            unreachable!();
        };
        m.match_type()
    }

    pub fn end(&mut self) {
        *self.guard = MatchState::NoMatch;
    }
}

pub struct MatchStateFacade {
    guard: std::sync::Arc<tokio::sync::Mutex<MatchState>>,
    fastforwarder: std::sync::Arc<parking_lot::Mutex<fastforwarder::Fastforwarder>>,
    config: std::sync::Arc<parking_lot::Mutex<config::Config>>,
}

impl MatchStateFacade {
    pub async fn lock(&self) -> MatchStateFacadeGuard<'_> {
        MatchStateFacadeGuard {
            guard: self.guard.lock().await,
            fastforwarder: self.fastforwarder.clone(),
            config: self.config.clone(),
        }
    }
}

struct InnerFacade {
    match_state: std::sync::Arc<tokio::sync::Mutex<MatchState>>,
    joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
    gui_state: std::sync::Arc<gui::State>,
    config: std::sync::Arc<parking_lot::Mutex<config::Config>>,
    fastforwarder: std::sync::Arc<parking_lot::Mutex<fastforwarder::Fastforwarder>>,
}

#[derive(Clone)]
pub struct Facade(std::rc::Rc<std::cell::RefCell<InnerFacade>>);

impl Facade {
    pub fn match_state(&mut self) -> MatchStateFacade {
        MatchStateFacade {
            guard: self.0.borrow().match_state.clone(),
            fastforwarder: self.0.borrow().fastforwarder.clone(),
            config: self.0.borrow().config.clone(),
        }
    }

    pub fn joyflags(&self) -> u32 {
        self.0
            .borrow()
            .joyflags
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn gui_state(&self) -> std::sync::Arc<gui::State> {
        self.0.borrow().gui_state.clone()
    }
}

impl Loaded {
    pub fn new(
        rom_filename: &std::path::Path,
        save_filename: &std::path::Path,
        handle: tokio::runtime::Handle,
        audio_device: &cpal::Device,
        config: Arc<Mutex<config::Config>>,
        gui_state: std::sync::Arc<gui::State>,
        vbuf: std::sync::Arc<Mutex<Vec<u8>>>,
        emu_tps_counter: std::sync::Arc<Mutex<tps::Counter>>,
    ) -> Result<Self, anyhow::Error> {
        let roms_path = std::path::Path::new("roms");
        let saves_path = std::path::Path::new("saves");

        let rom_path = roms_path.join(&rom_filename);
        let save_path = saves_path.join(&save_filename);

        let core = Arc::new(Mutex::new({
            let mut core = mgba::core::Core::new_gba("tango")?;
            core.enable_video_buffer();

            let rom_vf = mgba::vfile::VFile::open(&rom_path, mgba::vfile::flags::O_RDONLY)?;
            core.as_mut().load_rom(rom_vf)?;

            let save_vf = mgba::vfile::VFile::open(
                &save_path,
                mgba::vfile::flags::O_CREAT | mgba::vfile::flags::O_RDWR,
            )?;
            core.as_mut().load_save(save_vf)?;

            log::info!("loaded game: {}", core.as_ref().game_title());
            core
        }));

        let bn6 = {
            let core = core.clone();
            let core = core.lock();
            bn6::BN6::new(&core.as_ref().game_title()).unwrap()
        };

        let match_state = Arc::new(tokio::sync::Mutex::new(MatchState::NoMatch));

        let mut thread = {
            let core = core.clone();
            mgba::thread::Thread::new(core)
        };
        thread.start();

        let stream = {
            let core = core.clone();
            mgba::audio::open_stream(core, audio_device)?
        };
        stream.play()?;

        let joyflags = Arc::new(std::sync::atomic::AtomicU32::new(0));

        let trapper = {
            let core = core.clone();
            let mut core = core.lock();
            core.as_mut()
                .gba_mut()
                .sync_mut()
                .as_mut()
                .expect("sync")
                .set_fps_target(60.0);

            let fastforwarder = Arc::new(parking_lot::Mutex::new(
                fastforwarder::Fastforwarder::new(&rom_path, Box::new(bn6.clone()))?,
            ));

            bn6.install_main_hooks(
                config.clone(),
                core.as_mut(),
                handle,
                Facade(std::rc::Rc::new(std::cell::RefCell::new(InnerFacade {
                    match_state: match_state.clone(),
                    joyflags: joyflags.clone(),
                    config: config.clone(),
                    gui_state,
                    fastforwarder,
                }))),
            )
        };

        {
            let core = core.clone();
            let vbuf = vbuf;
            let emu_tps_counter = emu_tps_counter;
            thread.set_frame_callback(Some(Box::new(move || {
                // TODO: This sometimes causes segfaults when the game gets unloaded.
                let core = core.lock();
                let mut vbuf = vbuf.lock();
                vbuf.copy_from_slice(core.video_buffer().unwrap());
                for i in (0..vbuf.len()).step_by(4) {
                    vbuf[i + 3] = 0xff;
                }
                let mut emu_tps_counter = emu_tps_counter.lock();
                emu_tps_counter.mark();
            })));
        }

        Ok(Loaded {
            core,
            match_state,
            joyflags,
            _trapper: trapper,
            _thread: thread,
            _stream: stream,
        })
    }

    pub fn lock_core(&self) -> parking_lot::MutexGuard<mgba::core::Core> {
        self.core.lock()
    }

    pub async fn lock_match_state(&self) -> tokio::sync::MutexGuard<'_, MatchState> {
        self.match_state.lock().await
    }

    pub fn set_joyflags(&self, joyflags: u32) {
        self.joyflags
            .store(joyflags, std::sync::atomic::Ordering::Relaxed)
    }
}
