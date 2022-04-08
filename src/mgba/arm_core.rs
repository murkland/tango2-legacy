use super::c;

pub struct ARMCore {
    pub(super) ptr: *mut c::ARMCore,
}

impl ARMCore {
    pub(super) fn wrap(ptr: *mut c::ARMCore) -> ARMCore {
        ARMCore { ptr }
    }

    pub unsafe fn components_mut(&mut self) -> &mut [*mut c::mCPUComponent] {
        std::slice::from_raw_parts_mut(
            (*self.ptr).components,
            c::mCPUComponentType_CPU_COMPONENT_MAX as usize,
        )
    }

    pub fn gpr(&self, r: usize) -> i32 {
        unsafe { (*self.ptr).__bindgen_anon_1.__bindgen_anon_1 }.gprs[r]
    }

    pub fn set_gpr(&mut self, r: usize, v: i32) {
        return unsafe { (*self.ptr).__bindgen_anon_1.__bindgen_anon_1 }.gprs[r] = v;
    }

    pub fn thumb_write_pc(&mut self) {
        // uint32_t pc = cpu->gprs[ARM_PC] & -WORD_SIZE_THUMB;
        let mut pc = (self.gpr(c::ARM_PC as usize) & -(c::WordSize_WORD_SIZE_THUMB as i32)) as u32;
        // cpu->memory.setActiveRegion(cpu, pc);
        unsafe {
            (*self.ptr).memory.setActiveRegion.unwrap()(self.ptr, pc as u32);
        }
        // LOAD_16(cpu->prefetch[0], pc & cpu->memory.activeMask, cpu->memory.activeRegion);
        unsafe {
            (*self.ptr).prefetch[0] = *(((*self.ptr).memory.activeRegion as *const u8)
                .offset((pc & (*self.ptr).memory.activeMask) as isize)
                as *const u16) as u32;
        }
        // pc += WORD_SIZE_THUMB;
        pc += c::WordSize_WORD_SIZE_THUMB;
        // LOAD_16(cpu->prefetch[1], pc & cpu->memory.activeMask, cpu->memory.activeRegion);
        unsafe {
            (*self.ptr).prefetch[1] = *(((*self.ptr).memory.activeRegion as *const u8)
                .offset((pc & (*self.ptr).memory.activeMask) as isize)
                as *const u16) as u32;
        }
        // cpu->gprs[ARM_PC] = pc;
        self.set_gpr(c::ARM_PC as usize, pc as i32);
    }
}
