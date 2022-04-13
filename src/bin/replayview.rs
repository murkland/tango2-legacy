use std::io::Read;

use byteorder::ReadBytesExt;

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

const HEADER: &[u8] = b"TOOT";
const VERSION: u8 = 0x09;

fn main() -> Result<(), anyhow::Error> {
    let mut f = zstd::stream::read::Decoder::new(std::fs::File::open(
        std::env::args_os().nth(1).unwrap(),
    )?)?;

    let mut header = [0u8; 4];
    f.read(&mut header)?;
    if &header != HEADER {
        anyhow::bail!("invalid header");
    }

    if f.read_u8()? != VERSION {
        anyhow::bail!("invalid version");
    }

    let local_player_index = f.read_u8();

    let mut p1_init = vec![0u8; f.read_u32::<byteorder::LittleEndian>()? as usize];
    f.read_exact(&mut p1_init)?;

    let mut p2_init = vec![0u8; f.read_u32::<byteorder::LittleEndian>()? as usize];
    f.read_exact(&mut p2_init)?;

    let mut state = vec![0u8; f.read_u32::<byteorder::LittleEndian>()? as usize];
    f.read_exact(&mut state)?;
    let state = mgba::state::State::from_slice(&state);

    loop {
        let local_tick = f.read_u32::<byteorder::LittleEndian>()?;
        let remote_tick = f.read_u32::<byteorder::LittleEndian>()?;

        let p1_joyflags = f.read_u16::<byteorder::LittleEndian>()?;
        let p2_joyflags = f.read_u16::<byteorder::LittleEndian>()?;

        let p1_custom_scren_state = f.read_u8()?;
        let p2_custom_scren_state = f.read_u8()?;

        println!("local tick = {}, p1 joyflags = {:04x}, p2 joyflags = {:04x}, p1 custom screen = {}, p2 custom screen = {}", local_tick, p1_joyflags, p2_joyflags, p1_custom_scren_state, p2_custom_scren_state);

        let mut p1_turn = vec![0u8; f.read_u32::<byteorder::LittleEndian>()? as usize];
        f.read_exact(&mut p1_turn)?;

        let mut p2_turn = vec![0u8; f.read_u32::<byteorder::LittleEndian>()? as usize];
        f.read_exact(&mut p2_turn)?;
    }

    Ok(())
}
