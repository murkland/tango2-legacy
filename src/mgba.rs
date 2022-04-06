mod c {
    #![allow(non_upper_case_globals)]
    #![allow(non_camel_case_types)]
    #![allow(non_snake_case)]
    #![allow(dead_code)]
    include!(concat!(env!("OUT_DIR"), "/mgba_bindings.rs"));
}

pub struct Core {
    ptr: *mut c::mCore,
}

impl Core {
    pub fn new_gba() -> Option<Self> {
        let ptr = unsafe { c::GBACoreCreate() };
        if ptr.is_null() {
            None
        } else {
            unsafe {
                ptr.as_ref().unwrap().init.unwrap()(ptr);
            }
            Some(Core { ptr: ptr })
        }
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        unsafe { self.ptr.as_ref().unwrap().deinit.unwrap()(self.ptr) }
    }
}
