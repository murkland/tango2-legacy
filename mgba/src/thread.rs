use super::c;
use super::core;

#[repr(transparent)]
pub struct Thread(std::sync::Arc<parking_lot::Mutex<Box<ThreadImpl>>>);

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
    pub fn new(core: std::sync::Arc<parking_lot::Mutex<core::Core>>) -> Self {
        let core_ptr = core.lock().ptr;
        let mut t = Box::new(ThreadImpl {
            raw: unsafe { std::mem::zeroed::<c::mCoreThread>() },
            frame_callback: None,
        });
        t.raw.core = core_ptr;
        t.raw.logger.d = unsafe { *c::mLogGetContext() };
        t.raw.userData = &mut *t as *mut _ as *mut std::os::raw::c_void;
        t.raw.frameCallback = Some(c_frame_callback);
        Thread(std::sync::Arc::new(parking_lot::Mutex::new(t)))
    }

    pub fn set_frame_callback(&self, f: Option<Box<dyn Fn() + Send>>) {
        self.0.lock().frame_callback = f;
    }

    pub fn handle(&self) -> Handle {
        Handle {
            _thread_arc: self.0.clone(),
            ptr: &mut self.0.lock().raw,
        }
    }

    pub fn start(&self) -> bool {
        unsafe { c::mCoreThreadStart(&mut self.0.lock().raw) }
    }

    pub fn join(&self) {
        unsafe { c::mCoreThreadJoin(&mut self.0.lock().raw) }
    }

    pub fn end(&self) {
        unsafe { c::mCoreThreadEnd(&mut self.0.lock().raw) }
    }
}

#[derive(Clone)]
pub struct Handle {
    _thread_arc: std::sync::Arc<parking_lot::Mutex<Box<ThreadImpl>>>,
    ptr: *mut c::mCoreThread,
}

impl Handle {
    pub fn pause(&self) {
        unsafe { c::mCoreThreadPause(self.ptr) }
    }

    pub fn unpause(&self) {
        unsafe { c::mCoreThreadUnpause(self.ptr) }
    }
}
