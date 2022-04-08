use super::c;

pub struct Sync {
    pub(super) ptr: *mut c::mCoreSync,
}

impl<'a> Sync {
    pub(crate) fn wrap(ptr: *mut c::mCoreSync) -> Sync {
        Sync { ptr }
    }

    pub fn fps_target(&self) -> f32 {
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
