pub mod offsets;
use crate::mgba::core;

pub struct BN6 {
    pub offsets: offsets::Offsets,
}

impl BN6 {
    pub fn new(title: &str) -> Option<BN6> {
        let offsets = match offsets::get_offsets(title) {
            Some(o) => o,
            None => return None,
        };
        Some(BN6 { offsets })
    }

    pub fn get_local_joy_flags(&self, core: &core::Core) -> u16 {
        core.raw_read_16(self.offsets.ewram.joypad + 0x00, -1)
    }

    pub fn get_local_custom_screen_state(&self, core: &core::Core) -> u8 {
        core.raw_read_8(self.offsets.ewram.battle_state + 0x11, -1)
    }

    pub fn get_local_marshaled_battle_state(&self, core: &core::Core) -> Vec<u8> {
        core.raw_read_range(self.offsets.ewram.local_marshaled_battle_state, -1, 0x100)
    }

    pub fn set_player_input_state(
        &self,
        core: &core::Core,
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
        core: &core::Core,
        index: u32,
        marshaled: &Vec<u8>,
    ) {
        core.raw_write_range(
            self.offsets.ewram.player_marshaled_state_arr + index * 0x100,
            -1,
            marshaled,
        )
    }

    pub fn get_local_wins(&self, core: &core::Core) -> u8 {
        core.raw_read_8(self.offsets.ewram.battle_state + 0x18, -1)
    }

    pub fn get_remote_wins(&self, core: &core::Core) -> u8 {
        core.raw_read_8(self.offsets.ewram.battle_state + 0x19, -1)
    }

    pub fn get_rng2_state(&self, core: &core::Core) -> u32 {
        core.raw_read_32(self.offsets.ewram.rng2, -1)
    }

    pub fn get_menu_control_state(&self, core: &core::Core, i: u32) -> u32 {
        core.raw_read_32(self.offsets.ewram.menu_control + i, -1)
    }

    pub fn set_link_battle_settings_and_background(&self, core: &core::Core, v: u16) {
        core.raw_write_16(self.offsets.ewram.menu_control + 0x2a, -1, v)
    }

    pub fn get_match_type(&self, core: &core::Core) -> u16 {
        core.raw_read_16(self.offsets.ewram.menu_control + 0x12, -1)
    }

    pub fn get_in_battle_time(&self, core: &core::Core) -> u32 {
        core.raw_read_32(self.offsets.ewram.battle_state + 0x60, -1)
    }
}
