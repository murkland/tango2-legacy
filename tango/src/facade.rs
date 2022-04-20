use crate::{audio, battle, compat, config, gui, hooks, input, loaded};

pub struct BattleStateFacadeGuard<'a> {
    guard: tokio::sync::MutexGuard<'a, battle::BattleState>,
    in_progress: std::sync::Arc<battle::InProgress>,
    audio_save_state_holder: std::sync::Arc<parking_lot::Mutex<Option<mgba::state::State>>>,
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
        mut core: mgba::core::CoreMutRef<'_>,
        current_tick: u32,
        joyflags: u16,
        custom_screen_state: u8,
        turn: Vec<u8>,
    ) -> bool {
        let battle_number = self.guard.number;

        let battle = self
            .guard
            .battle
            .as_mut()
            .expect("attempted to get battle information while no battle was active!");

        let local_player_index = battle.local_player_index();
        let local_tick = current_tick + battle.local_delay();
        let remote_tick = battle.last_committed_remote_input().local_tick;

        if !battle.add_local_input(input::Input {
            local_tick,
            remote_tick,
            joyflags,
            custom_screen_state,
            turn: turn.clone(),
        }) {
            log::warn!("local input buffer overflow!");
            return false;
        }

        self.in_progress
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

        let (input_pairs, left) = battle.consume_and_peek_local();

        for ip in &input_pairs {
            battle
                .replay_writer()
                .write_input(local_player_index, ip)
                .expect("write input");
        }

        let committed_state = battle
            .committed_state()
            .as_ref()
            .expect("committed state")
            .clone();
        let last_committed_remote_input = battle.last_committed_remote_input();

        let (committed_state, dirty_state, last_input) = battle
            .fastforwarder()
            .fastforward(
                &committed_state,
                &input_pairs,
                last_committed_remote_input,
                &left,
            )
            .expect("fastforward");

        core.load_state(&dirty_state).expect("load dirty state");

        *self.audio_save_state_holder.lock() = Some(dirty_state);

        battle.set_committed_state(committed_state);
        battle.set_last_input(last_input);

        core.gba_mut()
            .sync_mut()
            .expect("set fps target")
            .set_fps_target((loaded::EXPECTED_FPS as i32 + battle.tps_adjustment()) as f32);

        true
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
            assert!(battle.add_local_input(input::Input {
                local_tick: current_tick + i,
                remote_tick: 0,
                joyflags: 0,
                custom_screen_state: 0,
                turn: vec![],
            }));
        }
        for i in 0..battle.remote_delay() {
            assert!(battle.add_remote_input(input::Input {
                local_tick: current_tick + i,
                remote_tick: 0,
                joyflags: 0,
                custom_screen_state: 0,
                turn: vec![],
            }));
        }
    }

    pub async fn send_init(&mut self, init: &[u8]) {
        let local_delay = self
            .guard
            .battle
            .as_ref()
            .expect("attempted to get battle information while no battle was active!")
            .local_delay();

        self.in_progress
            .transport()
            .await
            .expect("no transport")
            .send_init(self.guard.number, local_delay, init)
            .await
            .expect("send init");
        log::info!("sent local init: {:?}", init);
    }

    pub async fn receive_init(&mut self) -> Option<Vec<u8>> {
        let init = match self.in_progress.receive_remote_init().await {
            Some(init) => init,
            None => {
                return None;
            }
        };
        log::info!("received remote init: {:?}", init);

        if init.battle_number != self.guard.number {
            log::warn!(
                "expected battle number {} but got {}",
                self.guard.number,
                init.battle_number,
            )
        }

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
}

#[derive(Clone)]
pub struct InProgressFacade {
    arc: std::sync::Arc<battle::InProgress>,
    audio_save_state_holder: std::sync::Arc<parking_lot::Mutex<Option<mgba::state::State>>>,
    primary_mux_handle: audio::mux_stream::MuxHandle,
}

impl InProgressFacade {
    pub async fn lock_battle_state(&self) -> BattleStateFacadeGuard<'_> {
        let guard = self.arc.lock_battle_state().await;
        BattleStateFacadeGuard {
            in_progress: self.arc.clone(),
            guard,
            audio_save_state_holder: self.audio_save_state_holder.clone(),
        }
    }

    pub async fn start_battle(&self, core: mgba::core::CoreMutRef<'_>) {
        self.arc.start_battle(core).await.expect("start battle");
    }

    pub async fn end_battle(&self, mut core: mgba::core::CoreMutRef<'_>) {
        self.arc.end_battle().await;
        core.gba_mut()
            .sync_mut()
            .expect("sync")
            .set_fps_target(loaded::EXPECTED_FPS as f32);
        self.primary_mux_handle.switch();
    }

    pub async fn lock_rng(&self) -> tokio::sync::MappedMutexGuard<'_, rand_pcg::Mcg128Xsl64> {
        self.arc.lock_rng().await.expect("rng")
    }

    pub fn match_type(&self) -> u16 {
        self.arc.match_type()
    }
}

pub struct MatchStateFacadeGuard<'a> {
    guard: tokio::sync::MutexGuard<'a, Option<battle::Match>>,
    compat_list: compat::CompatList,
    audio_supported_config: cpal::SupportedStreamConfig,
    rom_path: std::path::PathBuf,
    hooks: &'static Box<dyn hooks::Hooks + Send + Sync>,
    audio_mux: audio::mux_stream::MuxStream,
    audio_save_state_holder: std::sync::Arc<parking_lot::Mutex<Option<mgba::state::State>>>,
    primary_mux_handle: audio::mux_stream::MuxHandle,
    config: std::sync::Arc<parking_lot::Mutex<config::Config>>,
}

impl<'a> MatchStateFacadeGuard<'a> {
    pub fn is_active(&self) -> bool {
        self.guard.is_some()
    }

    pub async fn in_progress(&self) -> Option<InProgressFacade> {
        let in_progress = match &*self.guard.as_ref().unwrap().lock_in_progress().await {
            Some(in_progress) => in_progress.clone(),
            None => {
                return None;
            }
        };
        Some(InProgressFacade {
            arc: in_progress,
            audio_save_state_holder: self.audio_save_state_holder.clone(),
            primary_mux_handle: self.primary_mux_handle.clone(),
        })
    }

    pub async fn is_aborted(&self) -> bool {
        match &*self.guard {
            None => false,
            Some(m) => m.lock_in_progress().await.is_none(),
        }
    }

    pub async fn poll_for_ready(&self) -> battle::NegotiationStatus {
        let m = if let Some(m) = &*self.guard {
            m
        } else {
            unreachable!();
        };
        m.poll_for_ready().await
    }

    pub async fn start(
        &mut self,
        core: mgba::core::CoreMutRef<'_>,
        handle: tokio::runtime::Handle,
        match_type: u16,
        s: gui::ConnectRequest,
    ) {
        let config = self.config.lock();
        let m = battle::Match::new(
            self.compat_list.clone(),
            self.audio_supported_config.clone(),
            self.rom_path.clone(),
            self.hooks,
            self.audio_mux.clone(),
            self.audio_save_state_holder.clone(),
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
        m.start(handle).await;
        *self.guard = Some(m);
    }

    pub async fn abort(&mut self, mut core: mgba::core::CoreMutRef<'_>) {
        core.gba_mut()
            .sync_mut()
            .expect("sync")
            .set_fps_target(loaded::EXPECTED_FPS as f32);
        self.primary_mux_handle.switch();
        *self.guard.as_mut().unwrap().lock_in_progress().await = None;
    }

    pub fn end(&mut self) {
        *self.guard = None;
    }
}

#[derive(Clone)]
pub struct MatchFacade {
    arc: std::sync::Arc<tokio::sync::Mutex<Option<battle::Match>>>,
    compat_list: compat::CompatList,
    audio_supported_config: cpal::SupportedStreamConfig,
    rom_path: std::path::PathBuf,
    hooks: &'static Box<dyn hooks::Hooks + Send + Sync>,
    audio_mux: audio::mux_stream::MuxStream,
    audio_save_state_holder: std::sync::Arc<parking_lot::Mutex<Option<mgba::state::State>>>,
    primary_mux_handle: audio::mux_stream::MuxHandle,
    config: std::sync::Arc<parking_lot::Mutex<config::Config>>,
}

impl MatchFacade {
    pub async fn lock(&self) -> MatchStateFacadeGuard<'_> {
        MatchStateFacadeGuard {
            guard: self.arc.lock().await,
            rom_path: self.rom_path.clone(),
            hooks: self.hooks,
            compat_list: self.compat_list.clone(),
            audio_supported_config: self.audio_supported_config.clone(),
            audio_mux: self.audio_mux.clone(),
            audio_save_state_holder: self.audio_save_state_holder.clone(),
            primary_mux_handle: self.primary_mux_handle.clone(),
            config: self.config.clone(),
        }
    }
}

struct InnerFacade {
    handle: tokio::runtime::Handle,
    compat_list: compat::CompatList,
    audio_supported_config: cpal::SupportedStreamConfig,
    rom_path: std::path::PathBuf,
    hooks: &'static Box<dyn hooks::Hooks + Send + Sync>,
    match_: std::sync::Arc<tokio::sync::Mutex<Option<battle::Match>>>,
    joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
    gui_state: std::sync::Arc<gui::State>,
    config: std::sync::Arc<parking_lot::Mutex<config::Config>>,
    audio_mux: audio::mux_stream::MuxStream,
    audio_save_state_holder: std::sync::Arc<parking_lot::Mutex<Option<mgba::state::State>>>,
    primary_mux_handle: audio::mux_stream::MuxHandle,
}

#[derive(Clone)]
pub struct Facade(std::rc::Rc<std::cell::RefCell<InnerFacade>>);

impl Facade {
    pub fn new(
        handle: tokio::runtime::Handle,
        compat_list: compat::CompatList,
        audio_supported_config: cpal::SupportedStreamConfig,
        rom_path: std::path::PathBuf,
        hooks: &'static Box<dyn hooks::Hooks + Send + Sync>,
        match_: std::sync::Arc<tokio::sync::Mutex<Option<battle::Match>>>,
        joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
        gui_state: std::sync::Arc<gui::State>,
        config: std::sync::Arc<parking_lot::Mutex<config::Config>>,
        audio_mux: audio::mux_stream::MuxStream,
        audio_save_state_holder: std::sync::Arc<parking_lot::Mutex<Option<mgba::state::State>>>,
        primary_mux_handle: audio::mux_stream::MuxHandle,
    ) -> Self {
        Self(std::rc::Rc::new(std::cell::RefCell::new(InnerFacade {
            handle,
            compat_list,
            audio_supported_config,
            rom_path,
            hooks,
            match_,
            joyflags,
            config,
            gui_state,
            audio_mux,
            audio_save_state_holder,
            primary_mux_handle,
        })))
    }
    pub fn match_(&mut self) -> MatchFacade {
        MatchFacade {
            arc: self.0.borrow().match_.clone(),
            compat_list: self.0.borrow().compat_list.clone(),
            audio_supported_config: self.0.borrow().audio_supported_config.clone(),
            rom_path: self.0.borrow().rom_path.clone(),
            hooks: self.0.borrow().hooks,
            audio_mux: self.0.borrow().audio_mux.clone(),
            audio_save_state_holder: self.0.borrow().audio_save_state_holder.clone(),
            primary_mux_handle: self.0.borrow().primary_mux_handle.clone(),
            config: self.0.borrow().config.clone(),
        }
    }

    pub fn joyflags(&self) -> u32 {
        self.0
            .borrow()
            .joyflags
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn request_connect(&mut self) -> gui::ConnectStatus {
        let handle = self.0.borrow().handle.clone();
        let match_ = self.match_();
        self.0.borrow().gui_state.request_connect(
            {
                let match_ = match_.clone();
                let handle = handle.clone();
                Box::new(move || {
                    handle.block_on(async {
                        let mut match_ = match_.lock().await;
                        match_.end();
                    });
                })
            },
            {
                let match_ = match_.clone();
                let handle = handle.clone();
                Box::new(move || {
                    handle.block_on(async {
                        let match_ = match_.lock().await;
                        if !match_.is_active() {
                            return None;
                        }
                        Some(match_.poll_for_ready().await)
                    })
                })
            },
        )
    }

    pub fn connect_dialog_is_open(&self) -> bool {
        self.0.borrow().gui_state.connect_dialog_is_open()
    }
}

#[derive(Clone)]
pub struct AudioFacade {
    audio_save_state_holder: std::sync::Arc<parking_lot::Mutex<Option<mgba::state::State>>>,
    local_player_index: u8,
}

impl AudioFacade {
    pub fn new(
        audio_save_state_holder: std::sync::Arc<parking_lot::Mutex<Option<mgba::state::State>>>,
        local_player_index: u8,
    ) -> Self {
        Self {
            audio_save_state_holder,
            local_player_index,
        }
    }

    pub fn take_audio_save_state(&self) -> Option<mgba::state::State> {
        self.audio_save_state_holder.lock().take()
    }

    pub fn local_player_index(&self) -> u8 {
        self.local_player_index
    }
}

impl Drop for AudioFacade {
    fn drop(&mut self) {
        *self.audio_save_state_holder.lock() = None;
    }
}
