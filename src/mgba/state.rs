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

    pub fn as_slice(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                &*self.0 as *const c::GBASerializedState as *const u8,
                std::mem::size_of::<c::GBASerializedState>(),
            )
        }
    }

    pub fn from_slice(slice: &[u8]) -> Self {
        unsafe {
            let layout = std::alloc::Layout::new::<c::GBASerializedState>();
            let ptr = std::alloc::alloc(layout);
            if ptr.is_null() {
                std::alloc::handle_alloc_error(layout);
            }
            let slice2 =
                std::slice::from_raw_parts_mut(ptr, std::mem::size_of::<c::GBASerializedState>());
            slice2.copy_from_slice(slice);
            Self(Box::from_raw(ptr as *mut _ as *mut c::GBASerializedState))
        }
    }
}
