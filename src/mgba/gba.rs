use super::arm_core;
use super::c;
use super::sync;

pub struct GBA {
    pub(super) ptr: *mut c::GBA,
    arm_core: arm_core::ARMCore,
    sync: Option<sync::Sync>,
}

impl GBA {
    pub(super) fn wrap(ptr: *mut c::GBA) -> GBA {
        let sync_ptr = unsafe { *ptr }.sync;
        GBA {
            ptr,
            arm_core: arm_core::ARMCore::wrap(unsafe { *ptr }.cpu),
            sync: if sync_ptr.is_null() {
                None
            } else {
                Some(sync::Sync::wrap(sync_ptr))
            },
        }
    }

    pub fn sync_mut(&mut self) -> &mut Option<sync::Sync> {
        &mut self.sync
    }

    pub fn cpu_mut(&mut self) -> &mut arm_core::ARMCore {
        &mut self.arm_core
    }

    pub fn cpu(&self) -> &arm_core::ARMCore {
        &self.arm_core
    }
}

pub fn audio_calculate_ratio(
    input_sample_rate: f32,
    desired_fps: f32,
    desired_sample_rate: f32,
) -> f32 {
    unsafe { c::GBAAudioCalculateRatio(input_sample_rate, desired_fps, desired_sample_rate) }
}
