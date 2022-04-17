use super::c;
use const_zero::const_zero;

lazy_static! {
    static ref MLOG_FILTER: send_wrapper::SendWrapper<parking_lot::Mutex<c::mLogFilter>> = {
        let mut ptr = unsafe { const_zero!(c::mLogFilter) };
        unsafe {
            c::mLogFilterInit(&mut ptr);
        }
        send_wrapper::SendWrapper::new(parking_lot::Mutex::new(ptr))
    };
    static ref MLOGGER: send_wrapper::SendWrapper<parking_lot::Mutex<c::mLogger>> =
        send_wrapper::SendWrapper::new(parking_lot::Mutex::new(c::mLogger {
            log: Some(c_log),
            filter: &mut *MLOG_FILTER.lock(),
        }));
    static ref LOG_FUNC: send_wrapper::SendWrapper<parking_lot::Mutex<Box<dyn Fn(i32, u32, String)>>> =
        send_wrapper::SendWrapper::new(parking_lot::Mutex::new(Box::new(
            &|category, _level, message| {
                let category_name =
                    unsafe { std::ffi::CStr::from_ptr(c::mLogCategoryName(category)) }
                        .to_str()
                        .unwrap();
                log::info!("{}: {}", category_name, message);
            }
        )));
}

unsafe extern "C" fn c_log<T>(
    _logger: *mut c::mLogger,
    category: i32,
    level: u32,
    fmt: *const std::os::raw::c_char,
    args: *mut T,
) {
    LOG_FUNC.lock().as_ref()(category, level, vsprintf::vsprintf(fmt, args).unwrap());
}

pub fn init() {
    unsafe {
        c::mLogSetDefaultLogger(&mut *MLOGGER.lock());
    }
}
