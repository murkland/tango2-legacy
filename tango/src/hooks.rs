use crate::{facade, fastforwarder};

pub trait Hooks {
    fn install_fastforwarder_hooks(
        &self,
        core: mgba::core::CoreMutRef,
        ff_state: fastforwarder::State,
    ) -> mgba::trapper::Trapper;

    fn install_main_hooks(
        &self,
        core: mgba::core::CoreMutRef,
        handle: tokio::runtime::Handle,
        facade: facade::Facade,
    ) -> mgba::trapper::Trapper;

    fn install_audio_hooks(
        &self,
        core: mgba::core::CoreMutRef,
        audio_state_receiver: std::sync::mpsc::Receiver<mgba::state::State>,
    ) -> mgba::trapper::Trapper;

    fn prepare_for_fastforward(&self, core: mgba::core::CoreMutRef);

    fn current_tick(&self, core: mgba::core::CoreMutRef) -> u32;
}
