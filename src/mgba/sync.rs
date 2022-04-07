use super::c;

pub struct Sync<'a> {
    pub(crate) _core: &'a super::core::Core,
    pub(crate) ptr: *mut c::mCoreSync,
}

impl<'a> Sync<'a> {
    pub fn get_fps_target(&self) -> f32 {
        unsafe { self.ptr.as_ref().unwrap().fpsTarget }
    }

    pub fn set_fps_target(&mut self, fps_target: f32) {
        unsafe {
            self.ptr.as_mut().unwrap().fpsTarget = fps_target;
        }
    }

    pub fn lock_audio(&mut self) {
        unsafe { c::mCoreSyncLockAudio(self.ptr) }
    }

    pub fn consume_audio(&mut self) {
        unsafe { c::mCoreSyncConsumeAudio(self.ptr) }
    }
}
