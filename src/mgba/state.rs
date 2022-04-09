use super::c;

#[repr(transparent)]
pub struct State(pub(super) Box<c::GBASerializedState>);

unsafe impl Send for State {}

impl State {
    pub fn rom_title(&self) -> String {
        let title = unsafe { &*(&self.0.title as *const [i8] as *const [u8]) };
        let cstr = match std::ffi::CString::new(title) {
            Ok(r) => r,
            Err(err) => {
                let nul_pos = err.nul_position();
                std::ffi::CString::new(&err.into_vec()[0..nul_pos]).unwrap()
            }
        };
        cstr.to_str().unwrap().to_string()
    }

    pub fn rom_crc32(&self) -> u32 {
        self.0.romCrc32
    }
}
