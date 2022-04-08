use super::c;

#[repr(transparent)]
pub struct SyncRef<'a>(pub(super) &'a *mut c::mCoreSync);

impl<'a> SyncRef<'a> {
    pub fn fps_target(&self) -> f32 {
        unsafe { self.0.as_ref().unwrap().fpsTarget }
    }
}

#[repr(transparent)]
pub struct SyncMutRef<'a>(pub(super) &'a mut *mut c::mCoreSync);

impl<'a> SyncMutRef<'a> {
    pub fn as_ref(&self) -> SyncRef {
        SyncRef(&*self.0)
    }

    pub fn set_fps_target(&mut self, fps_target: f32) {
        unsafe {
            (*self.0).as_mut().unwrap().fpsTarget = fps_target;
        }
    }

    pub fn lock_audio(&mut self) {
        unsafe {
            c::mCoreSyncLockAudio(*self.0);
        }
    }

    pub fn consume_audio(&mut self) {
        unsafe {
            c::mCoreSyncConsumeAudio(*self.0);
        }
    }
}
