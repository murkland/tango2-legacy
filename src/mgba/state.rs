use super::c;

pub struct State(pub(crate) c::GBASerializedState);

impl State {
    pub fn rom_title(&self) -> &str {
        unsafe {
            std::ffi::CStr::from_bytes_with_nul_unchecked(
                &*(&self.0.title as *const [i8] as *const [u8]),
            )
        }
        .to_str()
        .unwrap()
    }

    pub fn rom_crc32(&self) -> u32 {
        self.0.romCrc32
    }
}
