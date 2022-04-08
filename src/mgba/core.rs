use super::blip;
use super::c;
use super::gba;
use super::state;
use super::vfile;
use std::ffi::CString;

pub struct Core(pub(super) *mut c::mCore);

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

                (*ptr).init.unwrap()(ptr);
                let config_name_cstr = CString::new(config_name).unwrap();
                c::mCoreConfigInit(&mut ptr.as_mut().unwrap().config, config_name_cstr.as_ptr());
                c::mCoreConfigLoad(&mut ptr.as_mut().unwrap().config);
            }
            Some(Core(ptr))
        }
    }

    pub fn load_rom(&mut self, mut vf: vfile::VFile) -> bool {
        unsafe { (*self.0).loadROM.unwrap()(self.0, vf.release()) }
    }

    pub fn load_save(&mut self, mut vf: vfile::VFile) -> bool {
        unsafe { (*self.0).loadSave.unwrap()(self.0, vf.release()) }
    }

    pub fn run_frame(&mut self) {
        unsafe { (*self.0).runFrame.unwrap()(self.0) }
    }

    pub fn reset(&mut self) {
        unsafe { (*self.0).reset.unwrap()(self.0) }
    }

    pub fn get_audio_buffer_size(&self) -> u64 {
        unsafe { (*self.0).getAudioBufferSize.unwrap()(self.0) }
    }

    pub fn set_audio_buffer_size(&mut self, size: u64) {
        unsafe { (*self.0).setAudioBufferSize.unwrap()(self.0, size) }
    }

    pub fn get_audio_channel(&mut self, ch: i32) -> blip::Blip {
        blip::Blip(unsafe { (*self.0).getAudioChannel.unwrap()(self.0, ch) })
    }

    pub fn frequency(&mut self) -> i32 {
        unsafe { (*self.0).frequency.unwrap()(self.0) }
    }

    pub fn set_video_buffer(&mut self, buffer: &mut Vec<u8>, stride: u64) {
        unsafe {
            (*self.0).setVideoBuffer.unwrap()(self.0, buffer.as_mut_ptr() as *mut u32, stride)
        }
    }

    pub fn desired_video_dimensions(&self) -> (u32, u32) {
        let mut width: u32 = 0;
        let mut height: u32 = 0;
        unsafe { (*self.0).desiredVideoDimensions.unwrap()(self.0, &mut width, &mut height) };
        (width, height)
    }

    pub fn get_gba(&mut self) -> gba::GBA {
        gba::GBA(unsafe { (*self.0).board as *mut c::GBA })
    }

    pub fn save_state(&self) -> Option<state::State> {
        unsafe {
            let mut state = std::mem::zeroed::<state::State>();
            if (*self.0).saveState.unwrap()(
                self.0,
                &mut state.0 as *mut _ as *mut std::os::raw::c_void,
            ) {
                Some(state)
            } else {
                None
            }
        }
    }

    pub fn load_state(&mut self, state: &state::State) {
        unsafe {
            (*self.0).loadState.unwrap()(
                self.0,
                &state.0 as *const _ as *const std::os::raw::c_void,
            )
        };
    }

    pub fn set_keys(&mut self, keys: u32) {
        unsafe { (*self.0).setKeys.unwrap()(self.0, keys) }
    }

    pub fn raw_read_8(&self, address: u32, segment: i32) -> u8 {
        unsafe { (*self.0).rawRead8.unwrap()(self.0, address, segment) as u8 }
    }

    pub fn raw_read_16(&self, address: u32, segment: i32) -> u16 {
        unsafe { (*self.0).rawRead16.unwrap()(self.0, address, segment) as u16 }
    }

    pub fn raw_read_32(&self, address: u32, segment: i32) -> u32 {
        unsafe { (*self.0).rawRead32.unwrap()(self.0, address, segment) as u32 }
    }

    pub fn raw_write_8(&self, address: u32, segment: i32, v: u8) {
        unsafe { (*self.0).rawWrite8.unwrap()(self.0, address, segment, v) }
    }

    pub fn raw_write_16(&self, address: u32, segment: i32, v: u16) {
        unsafe { (*self.0).rawWrite16.unwrap()(self.0, address, segment, v) }
    }

    pub fn raw_write_32(&self, address: u32, segment: i32, v: u32) {
        unsafe { (*self.0).rawWrite32.unwrap()(self.0, address, segment, v) }
    }

    pub fn get_game_title(&self) -> String {
        let mut title = vec![0u8; 16];
        unsafe { (*self.0).getGameTitle.unwrap()(self.0, title.as_mut_ptr() as *mut _ as *mut i8) }
        let cstr = match std::ffi::CString::new(title) {
            Ok(r) => r,
            Err(err) => {
                let nul_pos = err.nul_position();
                std::ffi::CString::new(&err.into_vec()[0..nul_pos]).unwrap()
            }
        };
        cstr.to_str().unwrap().to_string()
    }

    pub fn get_game_code(&self) -> String {
        let mut code = vec![0u8; 12];
        unsafe { (*self.0).getGameCode.unwrap()(self.0, code.as_mut_ptr() as *mut _ as *mut i8) }
        let cstr = match std::ffi::CString::new(code) {
            Ok(r) => r,
            Err(err) => {
                let nul_pos = err.nul_position();
                std::ffi::CString::new(&err.into_vec()[0..nul_pos]).unwrap()
            }
        };
        cstr.to_str().unwrap().to_string()
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        unsafe {
            c::mCoreConfigDeinit(&mut self.0.as_mut().unwrap().config);
            (*self.0).deinit.unwrap()(self.0)
        }
    }
}
