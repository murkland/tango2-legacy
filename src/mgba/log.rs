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
            log: Some(mgba_mLogger_log),
            filter: &mut *MLOG_FILTER.lock().unwrap(),
        }));
    static ref LOG_FUNC: send_wrapper::SendWrapper<std::sync::Mutex<Option<Box<dyn Fn(i32, u32, String) -> ()>>>> =
        send_wrapper::SendWrapper::new(std::sync::Mutex::new(None));
}

#[allow(non_snake_case)]
unsafe extern "C" fn mgba_mLogger_log(
    _logger: *mut c::mLogger,
    category: i32,
    level: u32,
    fmt: *const i8,
    args: *mut i8,
) {
    LOG_FUNC.lock().unwrap().as_ref().unwrap()(
        category,
        level,
        vsprintf::vsprintf(fmt, args).unwrap(),
    );
}

pub fn set_default_logger(f: Box<dyn Fn(i32, u32, String) -> ()>) {
    *LOG_FUNC.lock().unwrap() = Some(f);
    unsafe {
        c::mLogSetDefaultLogger(&mut *MLOGGER.lock().unwrap());
    }
}
