use super::c;
use super::core;

pub struct Thread<'a> {
    raw: c::mCoreThread,
    pub frame_callback: Option<Box<dyn Fn() + Send + 'a>>,
}

unsafe extern "C" fn c_frame_callback(ptr: *mut c::mCoreThread) {
    let t = (*ptr).userData as *mut Thread;
    if let Some(cb) = &mut (*t).frame_callback {
        cb();
    }
}

impl<'a> Thread<'a> {
    pub fn new(core: std::sync::Arc<std::sync::Mutex<core::Core>>) -> Box<Self> {
        let core_ptr = core.lock().unwrap().0;
        let mut t = Box::new(Thread {
            raw: unsafe { std::mem::zeroed::<c::mCoreThread>() },
            frame_callback: None,
        });
        t.raw.core = core_ptr;
        t.raw.logger.d = unsafe { *c::mLogGetContext() };
        let user_data = &mut *t;
        t.raw.userData = user_data as *mut _ as *mut std::os::raw::c_void;
        t.raw.frameCallback = Some(c_frame_callback);
        t
    }

    pub fn start(&mut self) -> bool {
        unsafe { c::mCoreThreadStart(&mut self.raw) }
    }

    pub fn join(&mut self) {
        unsafe { c::mCoreThreadJoin(&mut self.raw) }
    }

    pub fn end(&mut self) {
        unsafe { c::mCoreThreadEnd(&mut self.raw) }
    }
}
