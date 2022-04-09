use super::blip;
use super::c;
use super::gba;
use super::state;
use super::vfile;
use std::ffi::CString;

#[repr(transparent)]
pub struct Core(pub(super) *mut c::mCore);

unsafe impl Send for Core {}

impl Core {
    pub fn new_gba(config_name: &str) -> anyhow::Result<Self> {
        let ptr = unsafe { c::GBACoreCreate() };
        if ptr.is_null() {
            anyhow::bail!("failed to create core");
        }
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
        Ok(Core(ptr))
    }

    pub fn load_rom(&mut self, mut vf: vfile::VFile) -> anyhow::Result<()> {
        if !unsafe { (*self.0).loadROM.unwrap()(self.0, vf.release()) } {
            anyhow::bail!("failed to load rom")
        }
        Ok(())
    }

    pub fn load_save(&mut self, mut vf: vfile::VFile) -> anyhow::Result<()> {
        if !unsafe { (*self.0).loadSave.unwrap()(self.0, vf.release()) } {
            anyhow::bail!("failed to load save")
        }
        Ok(())
    }

    pub fn run_frame(&mut self) {
        unsafe { (*self.0).runFrame.unwrap()(self.0) }
    }

    pub fn reset(&mut self) {
        unsafe { (*self.0).reset.unwrap()(self.0) }
    }

    pub fn audio_buffer_size(&self) -> u64 {
        unsafe { (*self.0).getAudioBufferSize.unwrap()(self.0) }
    }

    pub fn set_audio_buffer_size(&mut self, size: u64) {
        unsafe { (*self.0).setAudioBufferSize.unwrap()(self.0, size) }
    }

    pub fn audio_channel(&mut self, ch: i32) -> blip::BlipMutRef {
        blip::BlipMutRef {
            ptr: unsafe { (*self.0).getAudioChannel.unwrap()(self.0, ch) },
            _lifetime: std::marker::PhantomData,
        }
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

    pub fn gba_mut(&mut self) -> gba::GBAMutRef {
        gba::GBAMutRef {
            ptr: unsafe { (*self.0).board as *mut c::GBA },
            _lifetime: std::marker::PhantomData,
        }
    }

    pub fn gba(&self) -> gba::GBARef {
        gba::GBARef {
            ptr: unsafe { (*self.0).board as *const c::GBA },
            _lifetime: std::marker::PhantomData,
        }
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

    pub fn load_state(&mut self, state: &state::State) -> anyhow::Result<()> {
        if !unsafe {
            (*self.0).loadState.unwrap()(
                self.0,
                &state.0 as *const _ as *const std::os::raw::c_void,
            )
        } {
            anyhow::bail!("failed to load state");
        }
        Ok(())
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

    pub fn raw_read_range<const N: usize>(&self, address: u32, segment: i32) -> [u8; N] {
        let mut buf = [0; N];
        let ptr = buf.as_mut_ptr();
        for i in 0..N {
            unsafe {
                *ptr.add(i) = self.raw_read_8(address + i as u32, segment);
            }
        }
        buf
    }

    pub fn raw_write_8(&mut self, address: u32, segment: i32, v: u8) {
        unsafe { (*self.0).rawWrite8.unwrap()(self.0, address, segment, v) }
    }

    pub fn raw_write_16(&mut self, address: u32, segment: i32, v: u16) {
        unsafe { (*self.0).rawWrite16.unwrap()(self.0, address, segment, v) }
    }

    pub fn raw_write_32(&mut self, address: u32, segment: i32, v: u32) {
        unsafe { (*self.0).rawWrite32.unwrap()(self.0, address, segment, v) }
    }

    pub fn raw_write_range(&mut self, address: u32, segment: i32, buf: &[u8]) {
        for (i, v) in buf.iter().enumerate() {
            self.raw_write_8(address + i as u32, segment, *v);
        }
    }

    pub fn game_title(&self) -> String {
        let mut title = [0u8; 16];
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

    pub fn game_code(&self) -> String {
        let mut code = [0u8; 12];
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

    pub fn crc32(&self) -> u32 {
        let mut c: u32 = 0;
        unsafe {
            (*self.0).checksum.unwrap()(
                self.0,
                &mut c as *mut _ as *mut std::ffi::c_void,
                c::mCoreChecksumType_mCHECKSUM_CRC32,
            )
        };
        c
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
