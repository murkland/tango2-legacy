use crate::{battle, config, fastforwarder, gui, input, loaded};

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

    pub fn set_last_input(
        &mut self,
        input: input::Pair<input::Input>,
        mut core: mgba::core::CoreMutRef,
    ) {
        let battle = self
            .guard
            .battle
            .as_mut()
            .expect("attempted to get battle information while no battle was active!");
        battle.set_last_input(input);
        core.gba_mut()
            .sync_mut()
            .expect("sync")
            .set_fps_target((loaded::EXPECTED_FPS as i32 + battle.tps_adjustment()) as f32);
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
}

pub struct MatchStateFacadeGuard<'a> {
    guard: tokio::sync::MutexGuard<'a, loaded::MatchState>,
    fastforwarder: std::sync::Arc<parking_lot::Mutex<fastforwarder::Fastforwarder>>,
    config: std::sync::Arc<parking_lot::Mutex<config::Config>>,
}

impl<'a> MatchStateFacadeGuard<'a> {
    pub fn is_active(&self) -> bool {
        match &*self.guard {
            loaded::MatchState::NoMatch => false,
            loaded::MatchState::Aborted => false,
            loaded::MatchState::Match(_) => true,
        }
    }

    pub fn is_aborted(&self) -> bool {
        match &*self.guard {
            loaded::MatchState::NoMatch => false,
            loaded::MatchState::Aborted => true,
            loaded::MatchState::Match(_) => false,
        }
    }

    pub async fn poll_for_ready(&self) -> battle::NegotiationStatus {
        let m = if let loaded::MatchState::Match(m) = &*self.guard {
            m
        } else {
            unreachable!();
        };
        m.poll_for_ready().await
    }

    pub fn start(
        &mut self,
        core: mgba::core::CoreMutRef,
        handle: tokio::runtime::Handle,
        match_type: u16,
        s: gui::ConnectRequest,
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
        *self.guard = loaded::MatchState::Match(m);
    }

    pub fn abort(&mut self, mut core: mgba::core::CoreMutRef) {
        core.gba_mut()
            .sync_mut()
            .expect("sync")
            .set_fps_target(loaded::EXPECTED_FPS as f32);
        *self.guard = loaded::MatchState::Aborted;
    }

    pub async fn lock_battle_state(&'a self) -> BattleStateFacadeGuard<'a> {
        let m = if let loaded::MatchState::Match(m) = &*self.guard {
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
        let m = if let loaded::MatchState::Match(m) = &*self.guard {
            m
        } else {
            unreachable!();
        };
        m.start_battle().await;
    }

    pub async fn end_battle(&self, mut core: mgba::core::CoreMutRef<'_>) {
        let m = if let loaded::MatchState::Match(m) = &*self.guard {
            m
        } else {
            unreachable!();
        };
        core.gba_mut()
            .sync_mut()
            .expect("sync")
            .set_fps_target(loaded::EXPECTED_FPS as f32);
        m.end_battle().await;
    }

    pub async fn lock_rng(&self) -> tokio::sync::MappedMutexGuard<'_, rand_pcg::Mcg128Xsl64> {
        let m = if let loaded::MatchState::Match(m) = &*self.guard {
            m
        } else {
            unreachable!();
        };
        m.lock_rng().await.expect("rng")
    }

    pub fn match_type(&self) -> u16 {
        let m = if let loaded::MatchState::Match(m) = &*self.guard {
            m
        } else {
            unreachable!();
        };
        m.match_type()
    }

    pub fn end(&mut self) {
        *self.guard = loaded::MatchState::NoMatch;
    }
}

#[derive(Clone)]
pub struct MatchStateFacade {
    arc: std::sync::Arc<tokio::sync::Mutex<loaded::MatchState>>,
    fastforwarder: std::sync::Arc<parking_lot::Mutex<fastforwarder::Fastforwarder>>,
    config: std::sync::Arc<parking_lot::Mutex<config::Config>>,
}

impl MatchStateFacade {
    pub async fn lock(&self) -> MatchStateFacadeGuard<'_> {
        MatchStateFacadeGuard {
            guard: self.arc.lock().await,
            fastforwarder: self.fastforwarder.clone(),
            config: self.config.clone(),
        }
    }
}

struct InnerFacade {
    handle: tokio::runtime::Handle,
    match_state: std::sync::Arc<tokio::sync::Mutex<loaded::MatchState>>,
    joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
    gui_state: std::sync::Arc<gui::State>,
    config: std::sync::Arc<parking_lot::Mutex<config::Config>>,
    fastforwarder: std::sync::Arc<parking_lot::Mutex<fastforwarder::Fastforwarder>>,
}

#[derive(Clone)]
pub struct Facade(std::rc::Rc<std::cell::RefCell<InnerFacade>>);

impl Facade {
    pub fn new(
        handle: tokio::runtime::Handle,
        match_state: std::sync::Arc<tokio::sync::Mutex<loaded::MatchState>>,
        joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
        gui_state: std::sync::Arc<gui::State>,
        config: std::sync::Arc<parking_lot::Mutex<config::Config>>,
        fastforwarder: std::sync::Arc<parking_lot::Mutex<fastforwarder::Fastforwarder>>,
    ) -> Self {
        Self(std::rc::Rc::new(std::cell::RefCell::new(InnerFacade {
            handle,
            match_state,
            joyflags,
            config,
            gui_state,
            fastforwarder,
        })))
    }
    pub fn match_state(&mut self) -> MatchStateFacade {
        MatchStateFacade {
            arc: self.0.borrow().match_state.clone(),
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

    pub fn request_connect(&mut self) -> gui::ConnectStatus {
        let handle = self.0.borrow().handle.clone();
        let match_state = self.match_state();
        self.0.borrow().gui_state.request_connect(
            {
                let match_state = match_state.clone();
                let handle = handle.clone();
                Box::new(move || {
                    handle.block_on(async {
                        let mut match_state = match_state.lock().await;
                        match_state.end();
                    });
                })
            },
            {
                let match_state = match_state.clone();
                let handle = handle.clone();
                Box::new(move || {
                    handle.block_on(async {
                        let match_state = match_state.lock().await;
                        if !match_state.is_active() {
                            return None;
                        }
                        Some(match_state.poll_for_ready().await)
                    })
                })
            },
        )
    }

    pub fn connect_dialog_is_open(&self) -> bool {
        self.0.borrow().gui_state.connect_dialog_is_open()
    }
}
