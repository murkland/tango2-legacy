pub mod offsets;
use crate::mgba::core;

#[derive(Clone)]
pub struct BN6 {
    pub offsets: offsets::Offsets,
}

impl BN6 {
    pub fn new(title: &str) -> Option<BN6> {
        let offsets = match offsets::offsets(title) {
            Some(o) => o,
            None => return None,
        };
        Some(BN6 { offsets })
    }

    pub fn start_battle_from_comm_menu(&self, mut core: core::CoreMutRef) {
        core.raw_write_8(self.offsets.ewram.menu_control + 0x0, -1, 0x18);
        core.raw_write_8(self.offsets.ewram.menu_control + 0x1, -1, 0x18);
        core.raw_write_8(self.offsets.ewram.menu_control + 0x2, -1, 0x00);
        core.raw_write_8(self.offsets.ewram.menu_control + 0x3, -1, 0x00);
    }

    pub fn drop_matchmaking_from_comm_menu(&self, mut core: core::CoreMutRef, typ: u32) {
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

    pub fn local_joy_flags(&self, mut core: core::CoreMutRef) -> u16 {
        core.raw_read_16(self.offsets.ewram.joypad + 0x00, -1)
    }

    pub fn local_custom_screen_state(&self, mut core: core::CoreMutRef) -> u8 {
        core.raw_read_8(self.offsets.ewram.battle_state + 0x11, -1)
    }

    pub fn local_marshaled_battle_state(&self, mut core: core::CoreMutRef) -> [u8; 0x100] {
        core.raw_read_range(self.offsets.ewram.local_marshaled_battle_state, -1)
    }

    pub fn set_player_input_state(
        &self,
        mut core: core::CoreMutRef,
        index: u32,
        keys_pressed: u16,
        custom_screen_state: u8,
    ) {
        let a_player_input = self.offsets.ewram.player_input_data_arr + index * 0x08;
        let keys_held = core.raw_read_16(a_player_input + 0x02, -1);
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
        mut core: core::CoreMutRef,
        index: u32,
        marshaled: &[u8; 0x100],
    ) {
        core.raw_write_range(
            self.offsets.ewram.player_marshaled_state_arr + index * 0x100,
            -1,
            marshaled,
        )
    }

    pub fn local_wins(&self, mut core: core::CoreMutRef) -> u8 {
        core.raw_read_8(self.offsets.ewram.battle_state + 0x18, -1)
    }

    pub fn remote_wins(&self, mut core: core::CoreMutRef) -> u8 {
        core.raw_read_8(self.offsets.ewram.battle_state + 0x19, -1)
    }

    pub fn rng2_state(&self, mut core: core::CoreMutRef) -> u32 {
        core.raw_read_32(self.offsets.ewram.rng2, -1)
    }

    pub fn menu_control_state(&self, mut core: core::CoreMutRef, i: u32) -> u32 {
        core.raw_read_32(self.offsets.ewram.menu_control + i, -1)
    }

    pub fn set_link_battle_settings_and_background(&self, mut core: core::CoreMutRef, v: u16) {
        core.raw_write_16(self.offsets.ewram.menu_control + 0x2a, -1, v)
    }

    pub fn match_type(&self, mut core: core::CoreMutRef) -> u16 {
        core.raw_read_16(self.offsets.ewram.menu_control + 0x12, -1)
    }

    pub fn in_battle_time(&self, mut core: core::CoreMutRef) -> u32 {
        core.raw_read_32(self.offsets.ewram.battle_state + 0x60, -1)
    }
}
