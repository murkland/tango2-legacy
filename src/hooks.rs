use crate::{config, fastforwarder, gui, input, loaded};

pub trait FastforwarderState
where
    Self: FastforwarderStateClone,
{
    fn commit_time(&self) -> u32;
    fn set_committed_state(&self, state: mgba::state::State);
    fn dirty_time(&self) -> u32;
    fn set_dirty_state(&self, state: mgba::state::State);
    fn peek_input_pair(&self) -> Option<input::Pair<input::Input>>;
    fn pop_input_pair(&self) -> Option<input::Pair<input::Input>>;
    fn set_anyhow_error(&self, err: anyhow::Error);
    fn local_player_index(&self) -> u8;
}

pub trait FastforwarderStateClone {
    fn clone_box(&self) -> Box<dyn FastforwarderState>;
}

impl<T> FastforwarderStateClone for T
where
    T: 'static + FastforwarderState + Clone,
{
    fn clone_box(&self) -> Box<dyn FastforwarderState> {
        Box::new(self.clone())
    }
}

impl Clone for Box<dyn FastforwarderState> {
    fn clone(&self) -> Box<dyn FastforwarderState> {
        self.clone_box()
    }
}

pub trait Hooks {
    fn install_fastforwarder_hooks(
        &self,
        core: mgba::core::CoreMutRef,
        ff_state: Box<dyn FastforwarderState>,
    ) -> mgba::trapper::Trapper;
    fn install_main_hooks(
        &self,
        config: std::sync::Arc<parking_lot::Mutex<config::Config>>,
        core: mgba::core::CoreMutRef,
        handle: tokio::runtime::Handle,
        match_state: std::sync::Arc<tokio::sync::Mutex<loaded::MatchState>>,
        joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
        gui_state: std::sync::Weak<gui::State>,
        fastforwarder: fastforwarder::Fastforwarder,
    ) -> mgba::trapper::Trapper;
    fn prepare_for_fastforward(&self, core: mgba::core::CoreMutRef);
    fn in_battle_time(&self, core: mgba::core::CoreMutRef) -> u32;
}
