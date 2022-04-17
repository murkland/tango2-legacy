use super::c;
use super::core;

#[repr(transparent)]
pub struct Thread(std::sync::Arc<parking_lot::Mutex<Box<ThreadImpl>>>);

pub struct ThreadImpl {
    core: core::Core,
    raw: c::mCoreThread,
    frame_callback: Option<Box<dyn Fn(&[u8]) + Send + 'static>>,
    current_callback: std::cell::RefCell<Option<Box<dyn Fn(crate::core::CoreMutRef<'_>)>>>,
}

unsafe extern "C" fn c_frame_callback(ptr: *mut c::mCoreThread) {
    let t = &*((*ptr).userData as *mut ThreadImpl);
    if let Some(cb) = t.frame_callback.as_ref() {
        cb(t.core.video_buffer().unwrap());
    }
}

impl Thread {
    pub fn new(core: core::Core) -> Self {
        let core_ptr = core.ptr;
        let mut t = Box::new(ThreadImpl {
            core,
            raw: unsafe { std::mem::zeroed::<c::mCoreThread>() },
            frame_callback: None,
            current_callback: std::cell::RefCell::new(None),
        });
        t.raw.core = core_ptr;
        t.raw.logger.d = unsafe { *c::mLogGetContext() };
        t.raw.userData = &mut *t as *mut _ as *mut std::os::raw::c_void;
        t.raw.frameCallback = Some(c_frame_callback);
        Thread(std::sync::Arc::new(parking_lot::Mutex::new(t)))
    }

    pub fn set_frame_callback(&self, f: impl Fn(&[u8]) + Send + 'static) {
        self.0.lock().frame_callback = Some(Box::new(f));
    }

    pub fn handle(&self) -> Handle {
        Handle {
            thread: self.0.clone(),
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

    pub unsafe fn raw_core_ptr(&self) -> *mut c::mCore {
        self.0.lock().raw.core
    }
}

#[derive(Clone)]
pub struct Handle {
    thread: std::sync::Arc<parking_lot::Mutex<Box<ThreadImpl>>>,
    ptr: *mut c::mCoreThread,
}

unsafe extern "C" fn c_run_function(ptr: *mut c::mCoreThread) {
    let t = &mut *((*ptr).userData as *mut ThreadImpl);
    let mut cc = t.current_callback.borrow_mut();
    let cc = cc.as_mut().unwrap();
    cc(crate::core::CoreMutRef {
        ptr: t.raw.core,
        _lifetime: std::marker::PhantomData,
    });
}

impl Handle {
    pub fn pause(&self) {
        unsafe { c::mCoreThreadPause(self.ptr) }
    }

    pub fn unpause(&self) {
        unsafe { c::mCoreThreadUnpause(self.ptr) }
    }

    pub fn run_on_core(&self, f: impl Fn(crate::core::CoreMutRef<'_>) + Send + Sync + 'static) {
        let thread = self.thread.lock();
        *thread.current_callback.borrow_mut() = Some(Box::new(f));
        unsafe { c::mCoreThreadRunFunction(self.ptr, Some(c_run_function)) }
    }
}
