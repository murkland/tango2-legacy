use super::c;
use super::vfile;
use std::ffi::CString;

pub struct Core(*mut c::mCore);

impl Core {
    pub fn new_gba(config_name: &str) -> Option<Self> {
        let ptr = unsafe { c::GBACoreCreate() };
        if ptr.is_null() {
            None
        } else {
            unsafe {
                ptr.as_ref().unwrap().init.unwrap()(ptr);
                let config_name_cstr = CString::new(config_name).unwrap();
                c::mCoreConfigInit(&mut ptr.as_mut().unwrap().config, config_name_cstr.as_ptr());
                c::mCoreConfigLoad(&mut ptr.as_mut().unwrap().config);
            }
            Some(Core(ptr))
        }
    }

    pub fn load_rom(&mut self, mut vf: vfile::VFile) -> bool {
        unsafe { self.0.as_ref().unwrap().loadROM.unwrap()(self.0, vf.release()) }
    }

    pub fn run_frame(&mut self) {
        unsafe { self.0.as_ref().unwrap().runFrame.unwrap()(self.0) }
    }

    pub fn reset(&mut self) {
        unsafe { self.0.as_ref().unwrap().reset.unwrap()(self.0) }
    }

    pub fn set_video_buffer(&mut self, buffer: &mut Vec<u8>, stride: u64) {
        unsafe {
            self.0.as_ref().unwrap().setVideoBuffer.unwrap()(
                self.0,
                buffer.as_mut_ptr() as *mut u32,
                stride,
            )
        }
    }

    pub fn desired_video_dimensions(&self) -> (u32, u32) {
        let mut width: u32 = 0;
        let mut height: u32 = 0;
        unsafe {
            self.0.as_ref().unwrap().desiredVideoDimensions.unwrap()(
                self.0,
                &mut width,
                &mut height,
            );
        }
        (width, height)
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        unsafe {
            c::mCoreConfigDeinit(&mut self.0.as_mut().unwrap().config);
            self.0.as_ref().unwrap().deinit.unwrap()(self.0);
        }
    }
}
