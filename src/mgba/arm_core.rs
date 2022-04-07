use super::c;

pub struct ARMCore(pub(super) *mut c::ARMCore);

impl ARMCore {
    pub unsafe fn get_components_mut(&mut self) -> &mut [*mut c::mCPUComponent] {
        std::slice::from_raw_parts_mut(
            (*self.0).components,
            c::mCPUComponentType_CPU_COMPONENT_MAX as usize,
        )
    }

    pub fn get_gpr(&self, r: u32) -> i32 {
        return unsafe { (*self.0).__bindgen_anon_1.__bindgen_anon_1 }.gprs[r as usize];
    }

    pub fn set_gpr(&mut self, r: u32, v: i32) {
        unsafe { (*self.0).__bindgen_anon_1.__bindgen_anon_1 }.gprs[r as usize] = v;
    }

    pub fn thumb_write_pc(&mut self) {
        // uint32_t pc = cpu->gprs[ARM_PC] & -WORD_SIZE_THUMB;
        let mut pc = (self.get_gpr(c::ARM_PC) & -(c::WordSize_WORD_SIZE_THUMB as i32)) as u32;
        // cpu->memory.setActiveRegion(cpu, pc);
        unsafe {
            (*self.0).memory.setActiveRegion.unwrap()(self.0, pc as u32);
        }
        // LOAD_16(cpu->prefetch[0], pc & cpu->memory.activeMask, cpu->memory.activeRegion);
        unsafe {
            (*self.0).prefetch[0] = *(((*self.0).memory.activeRegion as *const u8)
                .offset((pc & (*self.0).memory.activeMask) as isize)
                as *const u16) as u32;
        }
        // pc += WORD_SIZE_THUMB;
        pc += c::WordSize_WORD_SIZE_THUMB;
        // LOAD_16(cpu->prefetch[1], pc & cpu->memory.activeMask, cpu->memory.activeRegion);
        unsafe {
            (*self.0).prefetch[1] = *(((*self.0).memory.activeRegion as *const u8)
                .offset((pc & (*self.0).memory.activeMask) as isize)
                as *const u16) as u32;
        }
        // cpu->gprs[ARM_PC] = pc;
        self.set_gpr(c::ARM_PC, pc as i32);
    }
}
