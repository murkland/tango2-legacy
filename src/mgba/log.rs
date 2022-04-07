use super::c;
use const_zero::const_zero;

lazy_static! {
    static ref MLOG_FILTER: send_wrapper::SendWrapper<std::sync::Mutex<c::mLogFilter>> = {
        let mut ptr = unsafe { const_zero!(c::mLogFilter) };
        unsafe {
            c::mLogFilterInit(&mut ptr);
        }
        send_wrapper::SendWrapper::new(std::sync::Mutex::new(ptr))
    };
    static ref MLOGGER: send_wrapper::SendWrapper<std::sync::Mutex<c::mLogger>> =
        send_wrapper::SendWrapper::new(std::sync::Mutex::new(c::mLogger {
            log: Some(c_log),
            filter: &mut *MLOG_FILTER.lock().unwrap(),
        }));
    static ref LOG_FUNC: send_wrapper::SendWrapper<std::sync::Mutex<Box<dyn Fn(i32, u32, String) -> ()>>> =
        send_wrapper::SendWrapper::new(std::sync::Mutex::new(Box::new(
            &|category, level, message| {
                let category_name =
                    unsafe { std::ffi::CStr::from_ptr(c::mLogCategoryName(category)) }
                        .to_str()
                        .unwrap();
                log::info!("{}: {}", category_name, message);
            }
        )));
}

unsafe extern "C" fn c_log(
    _logger: *mut c::mLogger,
    category: i32,
    level: u32,
    fmt: *const i8,
    args: *mut i8,
) {
    LOG_FUNC.lock().unwrap().as_ref()(category, level, vsprintf::vsprintf(fmt, args).unwrap());
}

pub fn init() {
    unsafe {
        c::mLogSetDefaultLogger(&mut *MLOGGER.lock().unwrap());
    }
}
