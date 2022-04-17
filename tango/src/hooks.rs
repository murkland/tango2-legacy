use crate::{facade, fastforwarder};

pub trait Hooks {
    fn get_fastforwarder_traps(
        &self,
        ff_state: fastforwarder::State,
    ) -> Vec<(u32, Box<dyn FnMut(mgba::core::CoreMutRef)>)>;

    fn get_primary_traps(
        &self,
        handle: tokio::runtime::Handle,
        facade: facade::Facade,
    ) -> Vec<(u32, Box<dyn FnMut(mgba::core::CoreMutRef)>)>;

    fn get_audio_traps(
        &self,
        audio_state_holder: std::sync::Arc<parking_lot::Mutex<Option<mgba::state::State>>>,
    ) -> Vec<(u32, Box<dyn FnMut(mgba::core::CoreMutRef)>)>;

    fn prepare_for_fastforward(&self, core: mgba::core::CoreMutRef);

    fn current_tick(&self, core: mgba::core::CoreMutRef) -> u32;
}
