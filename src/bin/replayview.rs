use byteorder::ReadBytesExt;
use clap::Parser;

#[derive(clap::Parser)]
struct Cli {
    #[clap(long)]
    dump: bool,

    #[clap(parse(from_os_str))]
    path: std::path::PathBuf,
}

#[derive(Debug)]
struct InputPair {
    local_tick: u32,
    remote_tick: u32,
    p1_input: Input,
    p2_input: Input,
}

#[derive(Debug)]
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

        let mut inits = [vec![], vec![]];
        for _ in 0..2 {
            let player_index = r.read_u8()?;
            let mut init = vec![0u8; r.read_u32::<byteorder::LittleEndian>()? as usize];
            r.read_exact(&mut init)?;
            inits[player_index as usize] = init;
        }
        let [p1_init, p2_init] = inits;

        let local_player_index = r.read_u8()?;

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
    env_logger::Builder::from_default_env()
        .filter(Some("tango"), log::LevelFilter::Info)
        .filter(Some("replayview"), log::LevelFilter::Info)
        .init();

    let args = Cli::parse();

    let mut f = zstd::stream::read::Decoder::new(std::fs::File::open(args.path)?)?;

    let replay = Replay::decode(&mut f)?;

    let rom_path = std::fs::read_dir("roms")?
        .flat_map(|dirent| {
            let dirent = dirent.as_ref().expect("dirent");
            let mut core = mgba::core::Core::new_gba("tango").expect("new_gba");
            let vf = match mgba::vfile::VFile::open(&dirent.path(), mgba::vfile::flags::O_RDONLY) {
                Ok(vf) => vf,
                Err(e) => {
                    log::warn!(
                        "failed to open {} for probing: {}",
                        dirent.path().display(),
                        e
                    );
                    return vec![];
                }
            };

            if let Err(e) = core.as_mut().load_rom(vf) {
                log::warn!(
                    "failed to load {} for probing: {}",
                    dirent.path().display(),
                    e
                );
                return vec![];
            }

            if core.as_ref().game_title() != replay.state.rom_title() {
                return vec![];
            }

            if core.as_ref().crc32() != replay.state.rom_crc32() {
                return vec![];
            }

            return vec![dirent.path().clone()];
        })
        .next()
        .ok_or_else(|| anyhow::format_err!("could not find eligible rom"))?;

    log::info!("found rom: {}", rom_path.display());

    let mut core = mgba::core::Core::new_gba("tango").expect("new_gba");
    let vf = mgba::vfile::VFile::open(&rom_path, mgba::vfile::flags::O_RDONLY).expect("vf");
    core.as_mut().load_rom(vf).expect("load_rom");

    if args.dump {
        for ip in &replay.input_pairs {
            println!("{:?}", ip);
        }
    }

    Ok(())
}
