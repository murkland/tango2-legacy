use clap::Parser;
use cpal::traits::{HostTrait, StreamTrait};
use tango::hooks::Hooks;

#[derive(clap::Parser)]
struct Cli {
    #[clap(long)]
    dump: bool,

    #[clap(parse(from_os_str))]
    path: Option<std::path::PathBuf>,
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

    let replay = tango::replay::Replay::decode(&mut f)?;
    log::info!(
        "replay is for {} (crc32 = {:08x})",
        replay.state.rom_title(),
        replay.state.rom_crc32()
    );

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
                log::warn!(
                    "{} is not eligible (title is {})",
                    dirent.path().display(),
                    core.as_ref().game_title()
                );
                return vec![];
            }

            if core.as_ref().crc32() != replay.state.rom_crc32() {
                log::warn!(
                    "{} is not eligible (crc32 is {:08x})",
                    dirent.path().display(),
                    core.as_ref().crc32()
                );
                return vec![];
            }

            return vec![dirent.path()];
        })
        .next()
        .ok_or_else(|| anyhow::format_err!("could not find eligible rom"))?;

    log::info!("found rom: {}", rom_path.display());

    let core = {
        let mut core = mgba::core::Core::new_gba("tango")?;
        let vf = mgba::vfile::VFile::open(&rom_path, mgba::vfile::flags::O_RDONLY)?;
        core.as_mut().load_rom(vf)?;
        core.enable_video_buffer();
        std::sync::Arc::new(parking_lot::Mutex::new(core))
    };

    let vbuf = std::sync::Arc::new(parking_lot::Mutex::new(vec![
        0u8;
        (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 4)
            as usize
    ]));

    let audio_device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| anyhow::format_err!("could not open audio device"))?;

    let mut thread = {
        let mut thread = mgba::thread::Thread::new(core.clone());
        let mut core = core.lock();
        thread.start();
        thread.pause();
        core.as_mut()
            .gba_mut()
            .sync_mut()
            .as_mut()
            .expect("sync")
            .set_fps_target(60.0);
        thread
    };

    {
        let core = core.clone();
        let vbuf = vbuf.clone();
        thread.set_frame_callback(Some(Box::new(move || {
            // TODO: This sometimes causes segfaults when the game gets unloaded.
            let core = core.lock();
            let mut vbuf = vbuf.lock();
            vbuf.copy_from_slice(core.video_buffer().unwrap());
            for i in (0..vbuf.len()).step_by(4) {
                vbuf[i + 3] = 0xff;
            }
        })));
    }

    let stream = tango::audio::open_stream(
        &audio_device,
        tango::audio::timewarp_stream::TimewarpStream::new(core.clone()),
    )?;
    stream.play()?;

    let event_loop = winit::event_loop::EventLoop::new();

    let window = {
        let size =
            winit::dpi::LogicalSize::new(mgba::gba::SCREEN_WIDTH * 3, mgba::gba::SCREEN_HEIGHT * 3);
        winit::window::WindowBuilder::new()
            .with_title("tango replayview")
            .with_inner_size(size)
            .with_min_inner_size(size)
            .build(&event_loop)?
    };

    let mut pixels = {
        let window_size = window.inner_size();
        let surface_texture =
            pixels::SurfaceTexture::new(window_size.width, window_size.height, &window);
        pixels::PixelsBuilder::new(
            mgba::gba::SCREEN_WIDTH,
            mgba::gba::SCREEN_HEIGHT,
            surface_texture,
        )
        .build()?
    };

    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let _trapper = {
        let mut core = core.lock();
        let done = done.clone();
        let hooks = tango::bn6::BN6::new(&core.as_ref().game_title()).unwrap();
        hooks.prepare_for_fastforward(core.as_mut());
        hooks.install_fastforwarder_hooks(
            core.as_mut(),
            tango::fastforwarder::State::new(
                replay.local_player_index,
                replay.input_pairs,
                0,
                0,
                Box::new(move || {
                    done.store(true, std::sync::atomic::Ordering::Relaxed);
                }),
            ),
        )
    };

    {
        let mut core = core.lock();
        core.as_mut().load_state(&replay.state)?;
        thread.unpause();
    }

    {
        let vbuf = vbuf.clone();
        event_loop.run(move |event, _, control_flow| {
            *control_flow = winit::event_loop::ControlFlow::Poll;

            if done.load(std::sync::atomic::Ordering::Relaxed) {
                *control_flow = winit::event_loop::ControlFlow::Exit;
                return;
            }

            match event {
                winit::event::Event::MainEventsCleared => {
                    let vbuf = vbuf.lock().clone();
                    pixels.get_frame().copy_from_slice(&vbuf);
                    pixels.render().expect("render pixels");
                }
                _ => {}
            }
        });
    }
}
