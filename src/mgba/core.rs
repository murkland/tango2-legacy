use super::blip;
use super::c;
use super::gba;
use super::vfile;
use std::ffi::CString;

pub struct Core(*mut c::mCore);

unsafe impl Send for Core {}

impl Core {
    pub fn new_gba(config_name: &str) -> Option<Self> {
        let ptr = unsafe { c::GBACoreCreate() };
        if ptr.is_null() {
            None
        } else {
            unsafe {
                {
                    // TODO: Make this more generic maybe.
                    let opts = &mut ptr.as_mut().unwrap().opts;
                    opts.sampleRate = 48000;
                    opts.videoSync = false;
                    opts.audioSync = true;
                }

                ptr.as_ref().unwrap().init.unwrap()(ptr);
                let config_name_cstr = CString::new(config_name).unwrap();
                c::mCoreConfigInit(&mut ptr.as_mut().unwrap().config, config_name_cstr.as_ptr());
                c::mCoreConfigLoad(&mut ptr.as_mut().unwrap().config);
            }
            Some(Core(ptr))
        }
    }

    pub unsafe fn as_ptr(&self) -> *const c::mCore {
        self.0
    }

    pub unsafe fn as_mut_ptr(&mut self) -> *mut c::mCore {
        self.0
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

    pub fn get_audio_buffer_size(&self) -> u64 {
        unsafe { self.0.as_ref().unwrap().getAudioBufferSize.unwrap()(self.0) }
    }

    pub fn set_audio_buffer_size(&mut self, size: u64) {
        unsafe { self.0.as_ref().unwrap().setAudioBufferSize.unwrap()(self.0, size) }
    }

    pub fn get_audio_channel(&mut self, ch: i32) -> blip::Blip {
        let ptr = unsafe { self.0.as_ref().unwrap().getAudioChannel.unwrap()(self.0, ch) };
        blip::Blip { _core: self, ptr }
    }

    pub fn frequency(&mut self) -> i32 {
        unsafe { self.0.as_ref().unwrap().frequency.unwrap()(self.0) }
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

    pub fn get_gba(&mut self) -> gba::GBA {
        let ptr = unsafe { self.0.as_ref().unwrap().board as *mut c::GBA };
        gba::GBA { core: self, ptr }
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
