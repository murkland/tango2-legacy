use super::c;
use super::sync;

pub struct GBA<'a> {
    pub(crate) core: &'a super::core::Core,
    pub(crate) ptr: *mut c::GBA,
}

impl<'a> GBA<'a> {
    pub fn get_sync(&mut self) -> Option<sync::Sync<'a>> {
        let ptr = unsafe { self.ptr.as_ref().unwrap().sync };
        if ptr.is_null() {
            None
        } else {
            Some(sync::Sync {
                _core: self.core,
                ptr,
            })
        }
    }
}

pub fn audio_calculate_ratio(
    input_sample_rate: f32,
    desired_fps: f32,
    desired_sample_rate: f32,
) -> f32 {
    unsafe { c::GBAAudioCalculateRatio(input_sample_rate, desired_fps, desired_sample_rate) }
}
