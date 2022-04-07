use super::c;

pub struct Sync(pub(super) *mut c::mCoreSync);

impl<'a> Sync {
    pub fn get_fps_target(&self) -> f32 {
        unsafe { self.0.as_ref().unwrap().fpsTarget }
    }

    pub fn set_fps_target(&mut self, fps_target: f32) {
        unsafe {
            self.0.as_mut().unwrap().fpsTarget = fps_target;
        }
    }

    pub fn lock_audio(&mut self) {
        unsafe { c::mCoreSyncLockAudio(self.0) }
    }

    pub fn consume_audio(&mut self) {
        unsafe { c::mCoreSyncConsumeAudio(self.0) }
    }
}
