use super::c;
use super::core;

pub struct Thread<'a> {
    _core: std::sync::Arc<std::sync::Mutex<core::Core>>,
    raw: c::mCoreThread,
    pub frame_callback: Option<Box<dyn FnMut() + Send + 'a>>,
}

#[allow(non_snake_case)]
unsafe extern "C" fn mgba_mCoreThread_frameCallback(ptr: *mut c::mCoreThread) {
    let t = (*ptr).userData as *mut Thread;
    if let Some(cb) = &mut (*t).frame_callback {
        cb();
    }
}

impl<'a> Thread<'a> {
    pub fn new(core: std::sync::Arc<std::sync::Mutex<core::Core>>) -> Self {
        let core_ptr = unsafe { core.lock().unwrap().as_mut_ptr() };
        let mut t = Thread {
            _core: core,
            raw: unsafe { std::mem::zeroed::<c::mCoreThread>() },
            frame_callback: None,
        };
        t.raw.core = core_ptr;
        t.raw.logger.d = unsafe { *c::mLogGetContext() };
        t.raw.userData = &mut t as *mut _ as *mut std::os::raw::c_void;
        t.raw.frameCallback = Some(mgba_mCoreThread_frameCallback);
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
