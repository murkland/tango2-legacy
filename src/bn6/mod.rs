use crate::hooks;

pub mod offsets;

#[derive(Clone)]
pub struct BN6 {
    pub offsets: offsets::Offsets,
    pub munger: Munger,
}

#[derive(Clone)]
pub struct Munger {
    offsets: offsets::Offsets,
}

impl Munger {
    pub fn start_battle_from_comm_menu(&self, mut core: mgba::core::CoreMutRef) {
        core.raw_write_8(self.offsets.ewram.menu_control + 0x0, -1, 0x18);
        core.raw_write_8(self.offsets.ewram.menu_control + 0x1, -1, 0x18);
        core.raw_write_8(self.offsets.ewram.menu_control + 0x2, -1, 0x00);
        core.raw_write_8(self.offsets.ewram.menu_control + 0x3, -1, 0x00);
    }

    pub fn drop_matchmaking_from_comm_menu(&self, mut core: mgba::core::CoreMutRef, typ: u32) {
        core.raw_write_8(self.offsets.ewram.menu_control + 0x0, -1, 0x18);
        core.raw_write_8(self.offsets.ewram.menu_control + 0x1, -1, 0x3c);
        core.raw_write_8(self.offsets.ewram.menu_control + 0x2, -1, 0x04);
        core.raw_write_8(self.offsets.ewram.menu_control + 0x3, -1, 0x04);
        if typ != 0 {
            let cpu = core.gba_mut().cpu_mut();
            cpu.set_gpr(0, typ as i32);
            cpu.set_pc(self.offsets.rom.comm_menu_run_chatbox_script_entry);
        }
    }

    pub fn local_custom_screen_state(&self, mut core: mgba::core::CoreMutRef) -> u8 {
        core.raw_read_8(self.offsets.ewram.battle_state + 0x11, -1)
    }

    pub fn local_marshaled_battle_state(&self, mut core: mgba::core::CoreMutRef) -> Vec<u8> {
        core.raw_read_range::<0x100>(self.offsets.ewram.local_marshaled_battle_state, -1)
            .to_vec()
    }

    pub fn set_player_input_state(
        &self,
        mut core: mgba::core::CoreMutRef,
        index: u32,
        keys_pressed: u16,
        custom_screen_state: u8,
    ) {
        let a_player_input = self.offsets.ewram.player_input_data_arr + index * 0x08;
        let keys_held = core.raw_read_16(a_player_input + 0x02, -1) | 0xfc00;
        core.raw_write_16(a_player_input + 0x02, -1, keys_pressed);
        core.raw_write_16(a_player_input + 0x04, -1, !keys_held & keys_pressed);
        core.raw_write_16(a_player_input + 0x06, -1, keys_held & !keys_pressed);
        core.raw_write_8(
            self.offsets.ewram.battle_state + 0x14 + index,
            -1,
            custom_screen_state,
        )
    }

    pub fn set_player_marshaled_battle_state(
        &self,
        mut core: mgba::core::CoreMutRef,
        index: u32,
        marshaled: &[u8],
    ) {
        core.raw_write_range(
            self.offsets.ewram.player_marshaled_state_arr + index * 0x100,
            -1,
            marshaled,
        )
    }

    pub fn set_link_battle_settings_and_background(
        &self,
        mut core: mgba::core::CoreMutRef,
        v: u16,
    ) {
        core.raw_write_16(self.offsets.ewram.menu_control + 0x2a, -1, v)
    }

    pub fn match_type(&self, mut core: mgba::core::CoreMutRef) -> u16 {
        core.raw_read_16(self.offsets.ewram.menu_control + 0x12, -1)
    }

    pub fn in_battle_time(&self, mut core: mgba::core::CoreMutRef) -> u32 {
        core.raw_read_32(self.offsets.ewram.battle_state + 0x60, -1)
    }
}

impl BN6 {
    pub fn new(title: &str) -> Option<BN6> {
        let offsets = match offsets::offsets(title) {
            Some(o) => o,
            None => return None,
        };
        Some(BN6 {
            offsets,
            munger: Munger { offsets },
        })
    }
}

pub fn random_battle_settings_and_background(rng: &mut impl rand::Rng, match_type: u8) -> u16 {
    const BATTLE_BACKGROUNDS: &[u16] = &[
        0x00, 0x01, 0x01, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f, 0x10, 0x11, 0x11, 0x13, 0x13,
    ];

    let lo = match match_type {
        0 => rng.gen_range(0..0x44u16),
        1 => rng.gen_range(0..0x60u16),
        2 => rng.gen_range(0..0x44u16) + 0x60u16,
        _ => 0u16,
    };

    let hi = BATTLE_BACKGROUNDS[rng.gen_range(0..BATTLE_BACKGROUNDS.len())];

    hi << 0x8 | lo
}

impl hooks::Hooks for BN6 {
    fn install_fastforwarder_hooks(
        &self,
        core: mgba::core::CoreMutRef,
        ff_state: Box<dyn hooks::FastforwarderState>,
    ) -> mgba::trapper::Trapper {
        mgba::trapper::Trapper::new(
            core,
            vec![
                {
                    let munger = self.munger.clone();
                    let ff_state = ff_state.clone();
                    (
                        self.offsets.rom.main_read_joyflags,
                        Box::new(move |mut core| {
                            let in_battle_time = munger.in_battle_time(core);

                            if in_battle_time == ff_state.commit_time() {
                                ff_state.set_committed_state(
                                    core.save_state().expect("save committed state"),
                                );
                            }

                            if in_battle_time == ff_state.dirty_time() {
                                ff_state
                                    .set_dirty_state(core.save_state().expect("save dirty state"));
                            }

                            let ip = match ff_state.peek_input_pair() {
                                Some(ip) => ip,
                                None => {
                                    return;
                                }
                            };

                            if ip.local.local_tick != ip.remote.local_tick {
                                ff_state.set_anyhow_error(anyhow::anyhow!(
                                    "p1 tick != p2 tick (in battle tick = {}): {} != {}",
                                    in_battle_time,
                                    ip.local.local_tick,
                                    ip.remote.local_tick
                                ));
                                return;
                            }

                            if ip.local.local_tick != in_battle_time {
                                ff_state.set_anyhow_error(anyhow::anyhow!(
                                    "input tick != in battle tick: {} != {}",
                                    ip.local.local_tick,
                                    in_battle_time,
                                ));
                                return;
                            }

                            core.gba_mut()
                                .cpu_mut()
                                .set_gpr(4, ip.local.joyflags as i32);
                        }),
                    )
                },
                {
                    let munger = self.munger.clone();
                    let ff_state = ff_state.clone();
                    (
                        self.offsets.rom.battle_update_call_battle_copy_input_data,
                        Box::new(move |mut core| {
                            let in_battle_time = munger.in_battle_time(core);

                            let ip = match ff_state.pop_input_pair() {
                                Some(ip) => ip,
                                None => {
                                    return;
                                }
                            };

                            core.gba_mut().cpu_mut().set_gpr(0, 0);
                            let r15 = core.as_ref().gba().cpu().gpr(15) as u32;
                            core.gba_mut().cpu_mut().set_pc(r15 + 4);

                            if ip.local.local_tick != ip.remote.local_tick {
                                ff_state.set_anyhow_error(anyhow::anyhow!(
                                    "p1 tick != p2 tick (in battle tick = {}): {} != {}",
                                    in_battle_time,
                                    ip.local.local_tick,
                                    ip.local.local_tick
                                ));
                                return;
                            }

                            if ip.local.local_tick != in_battle_time {
                                ff_state.set_anyhow_error(anyhow::anyhow!(
                                    "input tick != in battle tick: {} != {}",
                                    ip.local.local_tick,
                                    in_battle_time,
                                ));
                                return;
                            }

                            let local_player_index = ff_state.local_player_index();
                            let remote_player_index = 1 - local_player_index;

                            munger.set_player_input_state(
                                core,
                                local_player_index as u32,
                                ip.local.joyflags,
                                ip.local.custom_screen_state,
                            );
                            if !ip.local.turn.is_empty() {
                                munger.set_player_marshaled_battle_state(
                                    core,
                                    local_player_index as u32,
                                    ip.local.turn.as_slice(),
                                );
                            }

                            munger.set_player_input_state(
                                core,
                                remote_player_index as u32,
                                ip.remote.joyflags,
                                ip.remote.custom_screen_state,
                            );
                            if !ip.remote.turn.is_empty() {
                                munger.set_player_marshaled_battle_state(
                                    core,
                                    remote_player_index as u32,
                                    ip.remote.turn.as_slice(),
                                );
                            }
                        }),
                    )
                },
                {
                    let ff_state = ff_state.clone();
                    (
                        self.offsets.rom.battle_is_p2_tst,
                        Box::new(move |mut core| {
                            core.gba_mut()
                                .cpu_mut()
                                .set_gpr(0, ff_state.local_player_index() as i32);
                        }),
                    )
                },
                {
                    let ff_state = ff_state.clone();
                    (
                        self.offsets.rom.link_is_p2_ret,
                        Box::new(move |mut core| {
                            core.gba_mut()
                                .cpu_mut()
                                .set_gpr(0, ff_state.local_player_index() as i32);
                        }),
                    )
                },
                {
                    (
                        self.offsets
                            .rom
                            .comm_menu_in_battle_call_comm_menu_handle_link_cable_input,
                        Box::new(move |mut core| {
                            let r15 = core.as_ref().gba().cpu().gpr(15) as u32;
                            core.gba_mut().cpu_mut().set_pc(r15 + 4);
                        }),
                    )
                },
                {
                    (
                        self.offsets.rom.get_copy_data_input_state_ret,
                        Box::new(move |mut core| {
                            core.gba_mut().cpu_mut().set_gpr(0, 2);
                        }),
                    )
                },
            ],
        )
    }

    fn prepare_for_fastforward(&self, mut core: mgba::core::CoreMutRef) {
        core.gba_mut()
            .cpu_mut()
            .set_pc(self.offsets.rom.main_read_joyflags);
    }

    fn in_battle_time(&self, core: mgba::core::CoreMutRef) -> u32 {
        self.munger.in_battle_time(core)
    }
}
