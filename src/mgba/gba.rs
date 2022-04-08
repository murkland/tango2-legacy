use super::arm_core;
use super::c;
use super::sync;

#[repr(transparent)]
pub struct GBARef<'a> {
    pub(super) ptr: *const c::GBA,
    pub(super) _lifetime: std::marker::PhantomData<&'a ()>,
}

impl<'a> GBARef<'a> {
    pub fn cpu(&self) -> arm_core::ARMCoreRef<'a> {
        arm_core::ARMCoreRef {
            ptr: unsafe { (*self.ptr).cpu },
            _lifetime: self._lifetime,
        }
    }

    pub fn sync(&mut self) -> Option<sync::SyncRef> {
        let sync_ptr = unsafe { (*self.ptr).sync };
        if sync_ptr.is_null() {
            None
        } else {
            Some(sync::SyncRef {
                ptr: sync_ptr,
                _lifetime: self._lifetime,
            })
        }
    }
}

#[repr(transparent)]
pub struct GBAMutRef<'a> {
    pub(super) ptr: *mut c::GBA,
    pub(super) _lifetime: std::marker::PhantomData<&'a ()>,
}

impl<'a> GBAMutRef<'a> {
    pub fn as_ref(&self) -> GBARef {
        GBARef {
            ptr: self.ptr,
            _lifetime: self._lifetime,
        }
    }

    pub fn cpu_mut(&self) -> arm_core::ARMCoreMutRef<'a> {
        arm_core::ARMCoreMutRef {
            ptr: unsafe { (*self.ptr).cpu },
            _lifetime: self._lifetime,
        }
    }

    pub fn sync_mut(&mut self) -> Option<sync::SyncMutRef> {
        let sync_ptr = unsafe { (*self.ptr).sync };
        if sync_ptr.is_null() {
            None
        } else {
            Some(sync::SyncMutRef {
                ptr: sync_ptr,
                _lifetime: self._lifetime,
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
