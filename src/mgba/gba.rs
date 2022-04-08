use super::arm_core;
use super::c;
use super::sync;

#[repr(transparent)]
pub struct GBARef<'a>(pub(super) &'a *const c::GBA);

impl<'a> GBARef<'a> {
    pub fn cpu(&self) -> arm_core::ARMCoreRef<'a> {
        arm_core::ARMCoreRef::<'a>(unsafe { std::mem::transmute(&(**self.0).cpu) })
    }

    pub fn sync(&mut self) -> Option<sync::SyncRef> {
        if unsafe { (**self.0).sync.is_null() } {
            None
        } else {
            Some(sync::SyncRef(unsafe {
                std::mem::transmute(&(**self.0).sync)
            }))
        }
    }
}

#[repr(transparent)]
pub struct GBAMutRef<'a>(pub(super) &'a mut *mut c::GBA);

impl<'a> GBAMutRef<'a> {
    pub fn as_ref(&self) -> GBARef {
        GBARef(unsafe { std::mem::transmute(&*self.0) })
    }

    pub fn cpu_mut(&self) -> arm_core::ARMCoreMutRef<'a> {
        arm_core::ARMCoreMutRef::<'a>(unsafe { &mut (**self.0).cpu })
    }

    pub fn sync_mut(&mut self) -> Option<sync::SyncMutRef> {
        if unsafe { (**self.0).sync.is_null() } {
            None
        } else {
            Some(sync::SyncMutRef(unsafe { &mut (**self.0).sync }))
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
