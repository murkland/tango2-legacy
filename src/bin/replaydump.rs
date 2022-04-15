use byteorder::{ByteOrder, LittleEndian, ReadBytesExt};
use clap::Parser;
use std::io::{Read, Write};
use tango::hooks::Hooks;

#[derive(clap::Parser)]
struct Cli {
    #[clap(long)]
    dump: bool,

    #[clap(parse(from_os_str))]
    path: std::path::PathBuf,

    #[clap(parse(from_os_str))]
    output_path: Option<std::path::PathBuf>,
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

    let mut f = std::fs::File::open(args.path.clone())?;
    let output_path = args
        .output_path
        .unwrap_or_else(|| args.path.as_path().with_extension(".mp4").to_path_buf());

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

    let video_output = tempfile::NamedTempFile::new()?;
    let mut video_ffmpeg = std::process::Command::new("ffmpeg");
    video_ffmpeg.stdin(std::process::Stdio::piped());
    video_ffmpeg.args(&["-y"]);
    // Input args.
    video_ffmpeg.args(&[
        "-f",
        "rawvideo",
        "-pixel_format",
        "rgba",
        "-video_size",
        "240x160",
        "-framerate",
        "16777216/280896",
        "-i",
        "pipe:",
    ]);
    // Filters.
    video_ffmpeg.args(&["-vf", "scale=iw*5:ih*5:flags=neighbor"]);
    // Output args.
    video_ffmpeg.args(&["-c:v", "libx264", "-f", "mp4"]);
    video_ffmpeg.arg(&video_output.path());
    let mut video_child = video_ffmpeg.spawn()?;

    let audio_output = tempfile::NamedTempFile::new()?;
    let mut audio_ffmpeg = std::process::Command::new("ffmpeg");
    audio_ffmpeg.stdin(std::process::Stdio::piped());
    audio_ffmpeg.args(&["-y"]);
    // Input args.
    audio_ffmpeg.args(&["-f", "s16le", "-ar", "48k", "-ac", "2", "-i", "pipe:"]);
    // Output args.
    audio_ffmpeg.args(&["-c:a", "aac", "-f", "mp4"]);
    audio_ffmpeg.arg(&audio_output.path());
    let mut audio_child = audio_ffmpeg.spawn()?;

    const SAMPLE_RATE: f64 = 48000.0;
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

        vbuf.copy_from_slice(core.video_buffer().unwrap());
        for i in (0..vbuf.len()).step_by(4) {
            vbuf[i + 3] = 0xff;
        }
        video_child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(vbuf.as_slice())?;

        let mut audio_bytes = vec![0u8; samples.len() * 2];
        LittleEndian::write_i16_into(&samples, &mut audio_bytes[..]);
        audio_child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(&audio_bytes)?;
    }
    bar.finish();

    video_child.stdin = None;
    video_child.wait()?;
    audio_child.stdin = None;
    audio_child.wait()?;

    let mut mux_ffmpeg = std::process::Command::new("ffmpeg");
    mux_ffmpeg.args(&["-y"]);
    mux_ffmpeg.args(&["-i"]);
    mux_ffmpeg.arg(video_output.path());
    mux_ffmpeg.args(&["-i"]);
    mux_ffmpeg.arg(audio_output.path());
    mux_ffmpeg.args(&["-c:v", "copy", "-c:a", "copy"]);
    mux_ffmpeg.arg(output_path);
    let mut mux_child = mux_ffmpeg.spawn()?;
    mux_child.wait()?;

    Ok(())
}
