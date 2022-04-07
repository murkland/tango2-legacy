use super::c;
use std::ffi::CString;

pub struct VFile(*mut c::VFile);

impl VFile {
    pub fn open(path: &str, flags: i32) -> Option<Self> {
        let ptr = unsafe {
            let path_cstr = CString::new(path).unwrap();
            c::VFileOpen(path_cstr.as_ptr(), flags)
        };
        if ptr.is_null() {
            None
        } else {
            Some(VFile(ptr))
        }
    }

    pub(super) unsafe fn release(&mut self) -> *mut c::VFile {
        let ptr = self.0;
        self.0 = std::ptr::null_mut();
        ptr
    }
}

impl Drop for VFile {
    fn drop(&mut self) {
        if self.0.is_null() {
            return;
        }
        unsafe {
            self.0.as_ref().unwrap().close.unwrap()(self.0);
        }
    }
}
