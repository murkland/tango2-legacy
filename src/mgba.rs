use const_zero::const_zero;
use std::ffi::CString;

mod c {
    #![allow(non_upper_case_globals)]
    #![allow(non_camel_case_types)]
    #![allow(non_snake_case)]
    #![allow(dead_code)]
    include!(concat!(env!("OUT_DIR"), "/mgba_bindings.rs"));
}

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

pub struct Core(*mut c::mCore);

impl Core {
    pub fn new_gba() -> Option<Self> {
        let ptr = unsafe { c::GBACoreCreate() };
        if ptr.is_null() {
            None
        } else {
            unsafe {
                ptr.as_ref().unwrap().init.unwrap()(ptr);
                let config_name_cstr = CString::new("tango").unwrap();
                c::mCoreConfigInit(&mut ptr.as_mut().unwrap().config, config_name_cstr.as_ptr());
                c::mCoreConfigLoad(&mut ptr.as_mut().unwrap().config);
            }
            Some(Core(ptr))
        }
    }

    pub fn load_rom(&mut self, mut vf: VFile) -> bool {
        unsafe {
            let vf_ptr = vf.0;
            vf.0 = std::ptr::null_mut();
            self.0.as_ref().unwrap().loadROM.unwrap()(self.0, vf_ptr)
        }
    }

    pub fn run_frame(&mut self) {
        unsafe { self.0.as_ref().unwrap().runFrame.unwrap()(self.0) }
    }

    pub fn reset(&mut self) {
        unsafe { self.0.as_ref().unwrap().reset.unwrap()(self.0) }
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        unsafe { self.0.as_ref().unwrap().deinit.unwrap()(self.0) }
    }
}

unsafe extern "C" fn mgba_log_callback(
    logger: *mut c::mLogger,
    category: i32,
    level: u32,
    fmt: *const i8,
    args: *mut i8,
) {
}

static mut MLOG_FILTER: c::mLogFilter = unsafe { const_zero!(c::mLogFilter) };

lazy_static! {
    static ref MLOGGER: send_wrapper::SendWrapper<std::sync::Mutex<c::mLogger>> =
        send_wrapper::SendWrapper::new(std::sync::Mutex::new(c::mLogger {
            log: Some(mgba_log_callback),
            filter: unsafe { &mut MLOG_FILTER },
        }));
}

pub fn set_default_logger(f: &dyn Fn() -> ()) {
    unsafe {
        c::mLogFilterInit(&mut MLOG_FILTER);
        c::mLogSetDefaultLogger(&mut *MLOGGER.lock().unwrap());
    }
}
