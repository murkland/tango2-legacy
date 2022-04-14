use super::c;
use super::core;
use super::gba;

#[repr(transparent)]
pub struct Trapper(Box<TrapperCStruct>);

#[repr(C)]
struct TrapperCStruct {
    cpu_component: c::mCPUComponent,
    real_bkpt16: Option<unsafe extern "C" fn(*mut c::ARMCore, i32)>,
    r#impl: Impl,
}

struct Trap {
    handler: Box<dyn FnMut(core::CoreMutRef)>,
    original: u16,
}

struct Impl {
    traps: std::collections::HashMap<u32, Trap>,
    core_ptr: *mut c::mCore,
}

const TRAPPER_IMM: i32 = 0xef;

unsafe extern "C" fn c_trapper_init(
    _cpu: *mut std::os::raw::c_void,
    _cpu_component: *mut c::mCPUComponent,
) {
}

unsafe extern "C" fn c_trapper_deinit(_cpu_component: *mut c::mCPUComponent) {}

unsafe extern "C" fn c_trapper_bkpt16(arm_core: *mut c::ARMCore, imm: i32) {
    let gba = gba::GBAMutRef {
        ptr: (*arm_core).master as *mut c::GBA,
        _lifetime: std::marker::PhantomData,
    };
    let arm_core = gba.cpu_mut();
    let components = arm_core.components_mut();
    let trapper = components[c::mCPUComponentType_CPU_COMPONENT_MISC_1 as usize] as *mut _
        as *mut TrapperCStruct;
    if imm == TRAPPER_IMM {
        let r#impl = &mut (*trapper).r#impl;
        let caller = arm_core.as_ref().gpr(15) as u32 - c::WordSize_WORD_SIZE_THUMB * 2;
        let trap = r#impl.traps.get_mut(&caller).unwrap();
        c::ARMRunFake(arm_core.ptr, trap.original as u32);
        (trap.handler)(core::CoreMutRef {
            ptr: r#impl.core_ptr,
            _lifetime: std::marker::PhantomData,
        });
    }
    (*trapper).real_bkpt16.unwrap()(arm_core.ptr, imm)
}

impl Trapper {
    pub fn new(
        mut core: core::CoreMutRef,
        handlers: Vec<(u32, Box<dyn FnMut(core::CoreMutRef)>)>,
    ) -> Self {
        let mut cpu_component = unsafe { std::mem::zeroed::<c::mCPUComponent>() };
        cpu_component.init = Some(c_trapper_init);
        cpu_component.deinit = Some(c_trapper_deinit);
        let mut trapper_c_struct = Box::new(TrapperCStruct {
            cpu_component,
            real_bkpt16: None,
            r#impl: Impl {
                traps: std::collections::HashMap::new(),
                core_ptr: core.ptr,
            },
        });

        unsafe {
            let arm_core = core.gba_mut().cpu_mut().ptr;
            trapper_c_struct.real_bkpt16 = (*arm_core).irqh.bkpt16;
            let components = std::slice::from_raw_parts_mut(
                (*arm_core).components,
                c::mCPUComponentType_CPU_COMPONENT_MAX as usize,
            );
            components[c::mCPUComponentType_CPU_COMPONENT_MISC_1 as usize] =
                &mut *trapper_c_struct as *mut _ as *mut c::mCPUComponent;
            c::ARMHotplugAttach(arm_core, c::mCPUComponentType_CPU_COMPONENT_MISC_1 as u64);
            (*arm_core).irqh.bkpt16 = Some(c_trapper_bkpt16);
        }

        for (addr, handler) in handlers {
            match trapper_c_struct.r#impl.traps.entry(addr) {
                std::collections::hash_map::Entry::Occupied(_) => {
                    panic!("attempting to install a second trap at 0x{:08x}", addr);
                }
                std::collections::hash_map::Entry::Vacant(e) => {
                    let original = core.raw_read_16(addr, -1);
                    core.raw_write_16(addr, -1, (0xbe00 | TRAPPER_IMM) as u16);
                    e.insert(Trap { original, handler });
                }
            };
        }
        Trapper(trapper_c_struct)
    }
}
