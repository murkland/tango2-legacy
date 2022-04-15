use byteorder::{ByteOrder, LittleEndian, ReadBytesExt};
use clap::Parser;
use std::io::{Read, Write};
use tango::hooks::Hooks;

#[derive(clap::Parser)]
struct Cli {
    #[clap(long)]
    dump: bool,

    #[clap(parse(from_os_str))]
    path: Option<std::path::PathBuf>,
}

struct Replay {
    local_player_index: u8,
    state: mgba::state::State,
    input_pairs: Vec<tango::input::Pair<tango::input::Input>>,
}

const HEADER: &[u8] = b"TOOT";
const VERSION: u8 = 0x0a;

impl Replay {
    fn decode(mut r: impl std::io::Read) -> std::io::Result<Self> {
        let mut header = [0u8; 4];
        r.read_exact(&mut header)?;
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

        let mut zr = zstd::stream::read::Decoder::new(r)?;

        let local_player_index = zr.read_u8()?;

        let mut state = vec![0u8; zr.read_u32::<byteorder::LittleEndian>()? as usize];
        zr.read_exact(&mut state)?;
        let state = mgba::state::State::from_slice(&state);

        let mut input_pairs = vec![];

        loop {
            let local_tick = match zr.read_u32::<byteorder::LittleEndian>() {
                Ok(local_tick) => local_tick,
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::UnexpectedEof {
                        break;
                    }
                    return Err(e);
                }
            };
            let remote_tick = zr.read_u32::<byteorder::LittleEndian>()?;

            let p1_joyflags = zr.read_u16::<byteorder::LittleEndian>()?;
            let p2_joyflags = zr.read_u16::<byteorder::LittleEndian>()?;

            let p1_custom_screen_state = zr.read_u8()?;
            let p2_custom_screen_state = zr.read_u8()?;

            let mut p1_turn = vec![0u8; zr.read_u32::<byteorder::LittleEndian>()? as usize];
            zr.read_exact(&mut p1_turn)?;

            let mut p2_turn = vec![0u8; zr.read_u32::<byteorder::LittleEndian>()? as usize];
            zr.read_exact(&mut p2_turn)?;

            let p1_input = tango::input::Input {
                local_tick,
                remote_tick,
                joyflags: p1_joyflags,
                custom_screen_state: p1_custom_screen_state,
                turn: p1_turn,
            };

            let p2_input = tango::input::Input {
                local_tick,
                remote_tick: local_tick,
                joyflags: p2_joyflags,
                custom_screen_state: p2_custom_screen_state,
                turn: p2_turn,
            };

            let (local, remote) = if local_player_index == 0 {
                (p1_input, p2_input)
            } else {
                (p2_input, p1_input)
            };

            input_pairs.push(tango::input::Pair { local, remote });
        }

        Ok(Replay {
            local_player_index,
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
    mgba::log::init();

    let args = Cli::parse();

    let path = match args.path {
        Some(path) => path,
        None => native_dialog::FileDialog::new()
            .add_filter("tango replay", &["tangoreplay"])
            .show_open_single_file()?
            .ok_or_else(|| anyhow::anyhow!("no file selected"))?,
    };
    let mut f = std::fs::File::open(path)?;

    let replay = Replay::decode(&mut f)?;

    if args.dump {
        for ip in &replay.input_pairs {
            println!("{:?}", ip);
        }
    }

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

            return vec![dirent.path()];
        })
        .next()
        .ok_or_else(|| anyhow::format_err!("could not find eligible rom"))?;

    log::info!("found rom: {}", rom_path.display());

    let mut core = mgba::core::Core::new_gba("tango")?;
    core.enable_video_buffer();
    let vf = mgba::vfile::VFile::open(&rom_path, mgba::vfile::flags::O_RDONLY)?;
    core.as_mut().load_rom(vf)?;
    core.as_mut().reset();

    let done = std::rc::Rc::new(std::cell::RefCell::new(false));

    let ff_state = {
        let done = done.clone();
        tango::fastforwarder::State::new(
            replay.local_player_index,
            replay.input_pairs,
            0,
            0,
            Box::new(move || {
                *done.borrow_mut() = true;
            }),
        )
    };
    let hooks = tango::bn6::BN6::new(&core.as_ref().game_title()).unwrap();
    hooks.prepare_for_fastforward(core.as_mut());
    let _trapper = {
        let ff_state = ff_state.clone();
        hooks.install_fastforwarder_hooks(core.as_mut(), ff_state)
    };

    core.as_mut().load_state(&replay.state)?;

    let mut audio_out = std::fs::File::create("audio.pcm")?;
    let mut timecode_out = std::fs::File::create("timecodes.txt")?;
    let mut video_out = std::fs::File::create("video.rgba")?;

    const SAMPLE_RATE: f64 = 48000.0;
    let mut sample_time = 0;
    let mut samples = vec![0i16; SAMPLE_RATE as usize];
    let mut vbuf = vec![0u8; (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 4) as usize];
    let bar = indicatif::ProgressBar::new(ff_state.inputs_pairs_left() as u64);
    while !*done.borrow() {
        bar.inc(1);
        core.as_mut().run_frame();
        let clock_rate = core.as_ref().frequency();
        let n = {
            let mut core = core.as_mut();
            let mut left = core.audio_channel(0);
            left.set_rates(clock_rate as f64, SAMPLE_RATE);
            let n = left.samples_avail();
            left.read_samples(&mut samples[..(n * 2) as usize], left.samples_avail(), true);
            n
        };
        {
            let mut core = core.as_mut();
            let mut right = core.audio_channel(1);
            right.set_rates(clock_rate as f64, SAMPLE_RATE);
            right.read_samples(&mut samples[1..(n * 2) as usize], n, true);
        }
        let samples = &samples[..(n * 2) as usize];
        sample_time += n;

        let frame_time = std::time::Duration::from_secs_f64(sample_time as f64 / SAMPLE_RATE);

        let mut audio_bytes = vec![0u8; samples.len() * 2];
        LittleEndian::write_i16_into(&samples, &mut audio_bytes[..]);

        audio_out.write_all(&audio_bytes)?;
        timecode_out.write_all(format!("{}\n", frame_time.as_millis()).as_bytes())?;

        vbuf.copy_from_slice(core.video_buffer().unwrap());
        for i in (0..vbuf.len()).step_by(4) {
            vbuf[i + 3] = 0xff;
        }
        video_out.write_all(vbuf.as_slice())?;
    }
    bar.finish();

    Ok(())
}
