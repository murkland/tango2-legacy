use super::c;
use std::ffi::CString;

#[repr(transparent)]
pub struct VFile(*mut c::VFile);

pub mod flags {
    pub const O_RDONLY: u32 = super::c::O_RDONLY;
    pub const O_WRONLY: u32 = super::c::O_WRONLY;
    pub const O_RDWR: u32 = super::c::O_RDWR;
    pub const O_NONBLOCK: u32 = super::c::O_NONBLOCK;
    pub const O_APPEND: u32 = super::c::O_APPEND;
    pub const O_SYNC: u32 = super::c::O_SYNC;
    pub const O_SHLOCK: u32 = super::c::O_SHLOCK;
    pub const O_EXLOCK: u32 = super::c::O_EXLOCK;
    pub const O_ASYNC: u32 = super::c::O_ASYNC;
    pub const O_FSYNC: u32 = super::c::O_FSYNC;
    pub const O_NOFOLLOW: u32 = super::c::O_NOFOLLOW;
    pub const O_CREAT: u32 = super::c::O_CREAT;
    pub const O_TRUNC: u32 = super::c::O_TRUNC;
    pub const O_EXCL: u32 = super::c::O_EXCL;
    pub const O_EVTONLY: u32 = super::c::O_EVTONLY;
    pub const O_NOCTTY: u32 = super::c::O_NOCTTY;
    pub const O_DIRECTORY: u32 = super::c::O_DIRECTORY;
    pub const O_SYMLINK: u32 = super::c::O_SYMLINK;
    pub const O_DSYNC: u32 = super::c::O_DSYNC;
    pub const O_CLOEXEC: u32 = super::c::O_CLOEXEC;
    pub const O_NOFOLLOW_ANY: u32 = super::c::O_NOFOLLOW_ANY;
}

impl VFile {
    pub fn open(path: &str, flags: u32) -> anyhow::Result<Self> {
        let ptr = unsafe {
            let path_cstr = CString::new(path).unwrap();
            c::VFileOpen(path_cstr.as_ptr(), flags as i32)
        };
        if ptr.is_null() {
            anyhow::bail!("failed to open vfile")
        }
        Ok(VFile(ptr))
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
            (*self.0).close.unwrap()(self.0);
        }
    }
}
