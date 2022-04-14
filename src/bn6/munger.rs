#[derive(Clone)]
pub(super) struct Munger {
    pub(super) offsets: super::offsets::Offsets,
}

impl Munger {
    pub(super) fn start_battle_from_comm_menu(&self, mut core: mgba::core::CoreMutRef) {
        core.raw_write_8(self.offsets.ewram.menu_control + 0x0, -1, 0x18);
        core.raw_write_8(self.offsets.ewram.menu_control + 0x1, -1, 0x18);
        core.raw_write_8(self.offsets.ewram.menu_control + 0x2, -1, 0x00);
        core.raw_write_8(self.offsets.ewram.menu_control + 0x3, -1, 0x00);
    }

    pub(super) fn drop_matchmaking_from_comm_menu(
        &self,
        mut core: mgba::core::CoreMutRef,
        typ: u32,
    ) {
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

    pub(super) fn local_custom_screen_state(&self, mut core: mgba::core::CoreMutRef) -> u8 {
        core.raw_read_8(self.offsets.ewram.battle_state + 0x11, -1)
    }

    pub(super) fn local_marshaled_battle_state(&self, mut core: mgba::core::CoreMutRef) -> Vec<u8> {
        core.raw_read_range::<0x100>(self.offsets.ewram.local_marshaled_battle_state, -1)
            .to_vec()
    }

    pub(super) fn set_player_input_state(
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

    pub(super) fn set_player_marshaled_battle_state(
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

    pub(super) fn set_link_battle_settings_and_background(
        &self,
        mut core: mgba::core::CoreMutRef,
        v: u16,
    ) {
        core.raw_write_16(self.offsets.ewram.menu_control + 0x2a, -1, v)
    }

    pub(super) fn match_type(&self, mut core: mgba::core::CoreMutRef) -> u16 {
        core.raw_read_16(self.offsets.ewram.menu_control + 0x12, -1)
    }

    pub(super) fn in_battle_time(&self, mut core: mgba::core::CoreMutRef) -> u32 {
        core.raw_read_32(self.offsets.ewram.battle_state + 0x60, -1)
    }
}
