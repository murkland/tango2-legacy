use super::c;

pub struct ARMCore(pub(super) *mut c::ARMCore);

impl ARMCore {
    pub unsafe fn get_components_mut(&mut self) -> &mut [*mut c::mCPUComponent] {
        std::slice::from_raw_parts_mut(
            (*self.0).components,
            c::mCPUComponentType_CPU_COMPONENT_MAX as usize,
        )
    }

    pub fn get_gpr(&self, r: usize) -> i32 {
        return unsafe { (*self.0).__bindgen_anon_1.__bindgen_anon_1.gprs[r] };
    }

    pub fn set_gpr(&mut self, r: usize, v: i32) {
        unsafe { (*self.0).__bindgen_anon_1.__bindgen_anon_1.gprs[r] = v };
    }
}
