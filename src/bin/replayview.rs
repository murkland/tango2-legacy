struct InputPair {
    local_tick: u32,
    remote_tick: u32,
    p1_input: Input,
    p2_input: Input,
}

struct Input {
    joyflags: u16,
    custom_screen_state: u8,
    turn: Vec<u8>,
}

struct Replay {
    local_player_index: u8,
    p1_init: Vec<u8>,
    p2_init: Vec<u8>,
    state: mgba::state::State,
    input_pairs: Vec<InputPair>,
}

fn main() -> Result<(), anyhow::Error> {
    let f = std::fs::File::open(std::env::args_os().nth(1).unwrap())?;
    Ok(())
}
