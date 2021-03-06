#[derive(Clone, Copy)]
pub(super) struct EWRAMOffsets {
    pub(super) player_input_data_arr: u32,
    pub(super) battle_state: u32,
    pub(super) local_marshaled_battle_state: u32,
    pub(super) player_marshaled_state_arr: u32,
    pub(super) menu_control: u32,
}

#[derive(Clone, Copy)]
pub(super) struct ROMOffsets {
    pub(super) main_read_joyflags: u32,
    pub(super) get_copy_data_input_state_ret: u32,
    pub(super) battle_init_call_battle_copy_input_data: u32,
    pub(super) battle_update_call_battle_copy_input_data: u32,
    pub(super) battle_run_unpaused_step_cmp_retval: u32,
    pub(super) battle_init_marshal_ret: u32,
    pub(super) battle_turn_marshal_ret: u32,
    pub(super) battle_ending_ret: u32,
    pub(super) battle_start_ret: u32,
    pub(super) battle_end_entry: u32,
    pub(super) battle_is_p2_tst: u32,
    pub(super) link_is_p2_ret: u32,
    pub(super) comm_menu_init_battle_entry: u32,
    pub(super) comm_menu_handle_link_cable_input_entry: u32,
    pub(super) comm_menu_wait_for_friend_call_comm_menu_handle_link_cable_input: u32,
    pub(super) comm_menu_wait_for_friend_ret_cancel: u32,
    pub(super) comm_menu_in_battle_call_comm_menu_handle_link_cable_input: u32,
    pub(super) comm_menu_end_battle_entry: u32,
}

static EWRAM_OFFSETS: EWRAMOffsets = EWRAMOffsets {
    player_input_data_arr: 0x02036820,
    battle_state: 0x02034880,
    local_marshaled_battle_state: 0x0203cbe0,
    player_marshaled_state_arr: 0x0203f4a0,
    menu_control: 0x02009a30,
};

#[derive(Clone, Copy)]
pub struct Offsets {
    pub(super) rom: ROMOffsets,
    pub(super) ewram: EWRAMOffsets,
}

pub static MEGAMAN6_FXX: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        main_read_joyflags: 0x080003fa,
        get_copy_data_input_state_ret: 0x0801feec,
        battle_init_call_battle_copy_input_data: 0x08007902,
        battle_update_call_battle_copy_input_data: 0x08007a6e,
        battle_run_unpaused_step_cmp_retval: 0x08008102,
        battle_init_marshal_ret: 0x0800b2b8,
        battle_turn_marshal_ret: 0x0800b3d6,
        battle_start_ret: 0x08007304,
        battle_ending_ret: 0x0800951c,
        battle_end_entry: 0x08007ca0,
        battle_is_p2_tst: 0x0803dd52,
        link_is_p2_ret: 0x0803dd86,
        comm_menu_init_battle_entry: 0x0812b608,
        comm_menu_handle_link_cable_input_entry: 0x0803eae4,
        comm_menu_wait_for_friend_call_comm_menu_handle_link_cable_input: 0x08129f8a,
        comm_menu_wait_for_friend_ret_cancel: 0x08129fa4,
        comm_menu_in_battle_call_comm_menu_handle_link_cable_input: 0x0812b5ca,
        comm_menu_end_battle_entry: 0x0812b708,
    },
};

pub static MEGAMAN6_GXX: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        main_read_joyflags: 0x080003fa,
        get_copy_data_input_state_ret: 0x0801feec,
        battle_init_call_battle_copy_input_data: 0x08007902,
        battle_update_call_battle_copy_input_data: 0x08007a6e,
        battle_run_unpaused_step_cmp_retval: 0x08008102,
        battle_init_marshal_ret: 0x0800b2b8,
        battle_turn_marshal_ret: 0x0800b3d6,
        battle_start_ret: 0x08007304,
        battle_ending_ret: 0x0800951c,
        battle_end_entry: 0x08007ca0,
        battle_is_p2_tst: 0x0803dd26,
        link_is_p2_ret: 0x0803dd5a,
        comm_menu_init_battle_entry: 0x0812d3e4,
        comm_menu_handle_link_cable_input_entry: 0x0803eab8,
        comm_menu_wait_for_friend_call_comm_menu_handle_link_cable_input: 0x0812bd66,
        comm_menu_wait_for_friend_ret_cancel: 0x0812bd80,
        comm_menu_in_battle_call_comm_menu_handle_link_cable_input: 0x0812d3a6,
        comm_menu_end_battle_entry: 0x0812d4e4,
    },
};

pub static ROCKEXE6_RXX: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        main_read_joyflags: 0x080003fa,
        get_copy_data_input_state_ret: 0x08020300,
        battle_init_call_battle_copy_input_data: 0x080078ee,
        battle_update_call_battle_copy_input_data: 0x08007a6a,
        battle_run_unpaused_step_cmp_retval: 0x0800811a,
        battle_init_marshal_ret: 0x0800b8a0,
        battle_turn_marshal_ret: 0x0800b9be,
        battle_start_ret: 0x080072f8,
        battle_ending_ret: 0x080096ec,
        battle_end_entry: 0x08007c9c,
        battle_is_p2_tst: 0x0803ed96,
        link_is_p2_ret: 0x0803edca,
        comm_menu_init_battle_entry: 0x08134008,
        comm_menu_handle_link_cable_input_entry: 0x0803fb28,
        comm_menu_wait_for_friend_call_comm_menu_handle_link_cable_input: 0x0813299e,
        comm_menu_wait_for_friend_ret_cancel: 0x081329b8,
        comm_menu_in_battle_call_comm_menu_handle_link_cable_input: 0x08133fca,
        comm_menu_end_battle_entry: 0x08134108,
    },
};

pub static ROCKEXE6_GXX: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        main_read_joyflags: 0x080003fa,
        get_copy_data_input_state_ret: 0x08020300,
        battle_init_call_battle_copy_input_data: 0x080078ee,
        battle_update_call_battle_copy_input_data: 0x08007a6a,
        battle_run_unpaused_step_cmp_retval: 0x0800811a,
        battle_init_marshal_ret: 0x0800b8a0,
        battle_turn_marshal_ret: 0x0800b9be,
        battle_start_ret: 0x080072f8,
        battle_ending_ret: 0x080096ec,
        battle_end_entry: 0x08007c9c,
        battle_is_p2_tst: 0x0803ed6a,
        link_is_p2_ret: 0x0803ed9e,
        comm_menu_init_battle_entry: 0x08135dd0,
        comm_menu_handle_link_cable_input_entry: 0x0803fafc,
        comm_menu_wait_for_friend_call_comm_menu_handle_link_cable_input: 0x08134766,
        comm_menu_wait_for_friend_ret_cancel: 0x08134780,
        comm_menu_in_battle_call_comm_menu_handle_link_cable_input: 0x08135d92,
        comm_menu_end_battle_entry: 0x08135ed0,
    },
};
