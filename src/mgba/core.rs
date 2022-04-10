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

    pub fn as_ref(&self) -> CoreRef {
        CoreRef {
            ptr: self.0,
            _lifetime: std::marker::PhantomData,
        }
    }

    pub fn as_mut(&self) -> CoreMutRef {
        CoreMutRef {
            ptr: self.0,
            _lifetime: std::marker::PhantomData,
        }
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

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct CoreRef<'a> {
    pub(super) ptr: *const c::mCore,
    pub(super) _lifetime: std::marker::PhantomData<&'a ()>,
}

impl<'a> CoreRef<'a> {
    pub fn frequency(&self) -> i32 {
        unsafe { (*self.ptr).frequency.unwrap()(self.ptr) }
    }

    pub fn desired_video_dimensions(&self) -> (u32, u32) {
        let mut width: u32 = 0;
        let mut height: u32 = 0;
        unsafe { (*self.ptr).desiredVideoDimensions.unwrap()(self.ptr, &mut width, &mut height) };
        (width, height)
    }

    pub fn gba(&self) -> gba::GBARef {
        gba::GBARef {
            ptr: unsafe { (*self.ptr).board as *const c::GBA },
            _lifetime: std::marker::PhantomData,
        }
    }

    pub fn game_title(&self) -> String {
        let mut title = [0u8; 16];
        unsafe {
            (*self.ptr).getGameTitle.unwrap()(self.ptr, title.as_mut_ptr() as *mut _ as *mut i8)
        }
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
        unsafe {
            (*self.ptr).getGameCode.unwrap()(self.ptr, code.as_mut_ptr() as *mut _ as *mut i8)
        }
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
            (*self.ptr).checksum.unwrap()(
                self.ptr,
                &mut c as *mut _ as *mut std::ffi::c_void,
                c::mCoreChecksumType_mCHECKSUM_CRC32,
            )
        };
        c
    }
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct CoreMutRef<'a> {
    pub(super) ptr: *mut c::mCore,
    pub(super) _lifetime: std::marker::PhantomData<&'a ()>,
}

impl<'a> CoreMutRef<'a> {
    pub fn as_ref(&self) -> CoreRef {
        CoreRef {
            ptr: self.ptr,
            _lifetime: self._lifetime,
        }
    }

    pub fn gba_mut(&mut self) -> gba::GBAMutRef {
        gba::GBAMutRef {
            ptr: unsafe { (*self.ptr).board as *mut c::GBA },
            _lifetime: std::marker::PhantomData,
        }
    }

    pub fn load_rom(&mut self, mut vf: vfile::VFile) -> anyhow::Result<()> {
        if !unsafe { (*self.ptr).loadROM.unwrap()(self.ptr, vf.release()) } {
            anyhow::bail!("failed to load rom")
        }
        Ok(())
    }

    pub fn load_save(&mut self, mut vf: vfile::VFile) -> anyhow::Result<()> {
        if !unsafe { (*self.ptr).loadSave.unwrap()(self.ptr, vf.release()) } {
            anyhow::bail!("failed to load save")
        }
        Ok(())
    }

    pub fn load_state(&mut self, state: &state::State) -> anyhow::Result<()> {
        if !unsafe {
            (*self.ptr).loadState.unwrap()(
                self.ptr,
                &*state.0 as *const _ as *const std::os::raw::c_void,
            )
        } {
            anyhow::bail!("failed to load state");
        }
        Ok(())
    }

    pub fn save_state(&self) -> anyhow::Result<state::State> {
        unsafe {
            let layout = std::alloc::Layout::new::<c::GBASerializedState>();
            let ptr = std::alloc::alloc(layout);
            if ptr.is_null() {
                std::alloc::handle_alloc_error(layout);
            }
            let mut state =
                state::State(Box::from_raw(ptr as *mut _ as *mut c::GBASerializedState));
            if !(*self.ptr).saveState.unwrap()(
                self.ptr,
                &mut *state.0 as *mut _ as *mut std::os::raw::c_void,
            ) {
                anyhow::bail!("failed to save state");
            }
            Ok(state)
        }
    }

    pub fn set_keys(&mut self, keys: u32) {
        unsafe { (*self.ptr).setKeys.unwrap()(self.ptr, keys) }
    }

    pub fn raw_read_8(&mut self, address: u32, segment: i32) -> u8 {
        unsafe { (*self.ptr).rawRead8.unwrap()(self.ptr, address, segment) as u8 }
    }

    pub fn raw_read_16(&mut self, address: u32, segment: i32) -> u16 {
        unsafe { (*self.ptr).rawRead16.unwrap()(self.ptr, address, segment) as u16 }
    }

    pub fn raw_read_32(&mut self, address: u32, segment: i32) -> u32 {
        unsafe { (*self.ptr).rawRead32.unwrap()(self.ptr, address, segment) as u32 }
    }

    pub fn raw_read_range<const N: usize>(&mut self, address: u32, segment: i32) -> [u8; N] {
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
        unsafe { (*self.ptr).rawWrite8.unwrap()(self.ptr, address, segment, v) }
    }

    pub fn raw_write_16(&mut self, address: u32, segment: i32, v: u16) {
        unsafe { (*self.ptr).rawWrite16.unwrap()(self.ptr, address, segment, v) }
    }

    pub fn raw_write_32(&mut self, address: u32, segment: i32, v: u32) {
        unsafe { (*self.ptr).rawWrite32.unwrap()(self.ptr, address, segment, v) }
    }

    pub fn raw_write_range(&mut self, address: u32, segment: i32, buf: &[u8]) {
        for (i, v) in buf.iter().enumerate() {
            self.raw_write_8(address + i as u32, segment, *v);
        }
    }

    pub fn run_frame(&mut self) {
        unsafe { (*self.ptr).runFrame.unwrap()(self.ptr) }
    }

    pub fn reset(&mut self) {
        unsafe { (*self.ptr).reset.unwrap()(self.ptr) }
    }

    pub fn audio_buffer_size(&mut self) -> u64 {
        unsafe { (*self.ptr).getAudioBufferSize.unwrap()(self.ptr) }
    }

    pub fn set_audio_buffer_size(&mut self, size: u64) {
        unsafe { (*self.ptr).setAudioBufferSize.unwrap()(self.ptr, size) }
    }

    pub fn audio_channel(&mut self, ch: i32) -> blip::BlipMutRef {
        blip::BlipMutRef {
            ptr: unsafe { (*self.ptr).getAudioChannel.unwrap()(self.ptr, ch) },
            _lifetime: std::marker::PhantomData,
        }
    }

    pub fn set_video_buffer(&mut self, buffer: &mut Vec<u8>, stride: u64) {
        unsafe {
            (*self.ptr).setVideoBuffer.unwrap()(self.ptr, buffer.as_mut_ptr() as *mut u32, stride)
        }
    }
}
