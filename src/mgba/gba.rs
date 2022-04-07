use super::arm_core;
use super::c;
use super::sync;

pub struct GBA(pub(super) *mut c::GBA);

impl GBA {
    pub fn get_sync(&mut self) -> Option<sync::Sync> {
        let ptr = unsafe { *self.0 }.sync;
        if ptr.is_null() {
            None
        } else {
            Some(sync::Sync(ptr))
        }
    }

    pub fn get_cpu(&mut self) -> arm_core::ARMCore {
        arm_core::ARMCore(unsafe { *self.0 }.cpu)
    }
}

pub fn audio_calculate_ratio(
    input_sample_rate: f32,
    desired_fps: f32,
    desired_sample_rate: f32,
) -> f32 {
    unsafe { c::GBAAudioCalculateRatio(input_sample_rate, desired_fps, desired_sample_rate) }
}
