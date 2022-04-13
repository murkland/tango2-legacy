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

impl Replay {
    fn decode(r: &mut impl std::io::Read) -> std::io::Result<Self> {
        let mut header = [0u8; 4];
        r.read(&mut header)?;
        if &header != HEADER {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid header",
            ));
        }

        if r.read_u8()? != VERSION {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid version",
            ));
        }

        let local_player_index = r.read_u8()?;

        let mut p1_init = vec![0u8; r.read_u32::<byteorder::LittleEndian>()? as usize];
        r.read_exact(&mut p1_init)?;

        let mut p2_init = vec![0u8; r.read_u32::<byteorder::LittleEndian>()? as usize];
        r.read_exact(&mut p2_init)?;

        let mut state = vec![0u8; r.read_u32::<byteorder::LittleEndian>()? as usize];
        r.read_exact(&mut state)?;
        let state = mgba::state::State::from_slice(&state);

        let mut input_pairs = vec![];

        loop {
            let local_tick = match r.read_u32::<byteorder::LittleEndian>() {
                Ok(local_tick) => local_tick,
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::UnexpectedEof {
                        break;
                    }
                    return Err(e.into());
                }
            };
            let remote_tick = r.read_u32::<byteorder::LittleEndian>()?;

            let p1_joyflags = r.read_u16::<byteorder::LittleEndian>()?;
            let p2_joyflags = r.read_u16::<byteorder::LittleEndian>()?;

            let p1_custom_screen_state = r.read_u8()?;
            let p2_custom_screen_state = r.read_u8()?;

            let mut p1_turn = vec![0u8; r.read_u32::<byteorder::LittleEndian>()? as usize];
            r.read_exact(&mut p1_turn)?;

            let mut p2_turn = vec![0u8; r.read_u32::<byteorder::LittleEndian>()? as usize];
            r.read_exact(&mut p2_turn)?;

            input_pairs.push(InputPair {
                local_tick,
                remote_tick,
                p1_input: Input {
                    joyflags: p1_joyflags,
                    custom_screen_state: p1_custom_screen_state,
                    turn: p1_turn,
                },
                p2_input: Input {
                    joyflags: p2_joyflags,
                    custom_screen_state: p2_custom_screen_state,
                    turn: p2_turn,
                },
            });
        }

        Ok(Replay {
            local_player_index,
            p1_init,
            p2_init,
            state,
            input_pairs,
        })
    }
}

fn main() -> Result<(), anyhow::Error> {
    let mut f = zstd::stream::read::Decoder::new(std::fs::File::open(
        std::env::args_os().nth(1).unwrap(),
    )?)?;

    let replay = Replay::decode(&mut f)?;

    Ok(())
}
