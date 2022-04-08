use super::c;
use super::core;

#[repr(transparent)]
pub struct Thread(Box<ThreadImpl>);

pub struct ThreadImpl {
    raw: c::mCoreThread,
    frame_callback: Option<Box<dyn Fn() + Send>>,
}

unsafe extern "C" fn c_frame_callback(ptr: *mut c::mCoreThread) {
    let t = (*ptr).userData as *mut ThreadImpl;
    if let Some(cb) = &mut (*t).frame_callback {
        cb();
    }
}

impl Thread {
    pub fn new(core: std::sync::Arc<std::sync::Mutex<core::Core>>) -> Self {
        let core_ptr = core.lock().unwrap().0;
        let mut t = Box::new(ThreadImpl {
            raw: unsafe { std::mem::zeroed::<c::mCoreThread>() },
            frame_callback: None,
        });
        t.raw.core = core_ptr;
        t.raw.logger.d = unsafe { *c::mLogGetContext() };
        t.raw.userData = &mut *t as *mut _ as *mut std::os::raw::c_void;
        t.raw.frameCallback = Some(c_frame_callback);
        Thread(t)
    }

    pub fn set_frame_callback(&mut self, f: Option<Box<dyn Fn() + Send>>) {
        self.0.frame_callback = f;
    }

    pub fn start(&mut self) -> bool {
        unsafe { c::mCoreThreadStart(&mut self.0.raw) }
    }

    pub fn join(&mut self) {
        unsafe { c::mCoreThreadJoin(&mut self.0.raw) }
    }

    pub fn end(&mut self) {
        unsafe { c::mCoreThreadEnd(&mut self.0.raw) }
    }
}
