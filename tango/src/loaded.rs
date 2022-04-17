use crate::{audio, battle, bn6, config, facade, fastforwarder, gui, hooks::Hooks, tps};
use cpal::traits::StreamTrait;
use parking_lot::Mutex;
use std::sync::Arc;

pub const EXPECTED_FPS: u32 = 60;

pub enum MatchState {
    NoMatch,
    Aborted,
    Match(battle::Match),
}

pub struct Loaded {
    _stream: cpal::Stream,
    match_state: Arc<tokio::sync::Mutex<MatchState>>,
    joyflags: Arc<std::sync::atomic::AtomicU32>,
    _audio_core_thread: mgba::thread::Thread,
    thread: mgba::thread::Thread,
}

impl Loaded {
    pub fn new(
        rom_filename: &std::path::Path,
        save_filename: &std::path::Path,
        handle: tokio::runtime::Handle,
        audio_device: &cpal::Device,
        config: Arc<Mutex<config::Config>>,
        gui_state: std::sync::Arc<gui::State>,
        vbuf: std::sync::Arc<Mutex<Vec<u8>>>,
        emu_tps_counter: std::sync::Arc<Mutex<tps::Counter>>,
    ) -> Result<Self, anyhow::Error> {
        let roms_path = std::path::Path::new("roms");
        let saves_path = std::path::Path::new("saves");

        let rom_path = roms_path.join(&rom_filename);
        let save_path = saves_path.join(&save_filename);

        let mut core = mgba::core::Core::new_gba("tango")?;
        core.enable_video_buffer();

        let rom_vf = mgba::vfile::VFile::open(&rom_path, mgba::vfile::flags::O_RDONLY)?;
        core.as_mut().load_rom(rom_vf)?;

        let save_vf = mgba::vfile::VFile::open(
            &save_path,
            mgba::vfile::flags::O_CREAT | mgba::vfile::flags::O_RDWR,
        )?;
        core.as_mut().load_save(save_vf)?;

        log::info!("loaded game: {}", core.as_ref().game_title());

        let bn6 = bn6::BN6::new(&core.as_ref().game_title()).unwrap();

        let match_state = Arc::new(tokio::sync::Mutex::new(MatchState::NoMatch));

        let emu_tps_counter = emu_tps_counter;

        let joyflags = Arc::new(std::sync::atomic::AtomicU32::new(0));

        let audio_state_holder = Arc::new(parking_lot::Mutex::new(None));

        let mut audio_core = mgba::core::Core::new_gba("tango")?;
        let rom_vf = mgba::vfile::VFile::open(&rom_path, mgba::vfile::flags::O_RDONLY)?;
        audio_core.as_mut().load_rom(rom_vf)?;
        audio_core.as_mut().reset();

        audio_core.set_traps(bn6.get_audio_traps(audio_state_holder.clone()));

        let supported_config = audio::get_supported_config(audio_device)?;
        log::info!("selected audio config: {:?}", supported_config);

        let mut muxer = audio::mux_stream::MuxStream::new();

        let audio_core_mux_handle = muxer.add(audio::timewarp_stream::TimewarpStream::new(
            &audio_core,
            supported_config.sample_rate(),
            supported_config.channels(),
        ));

        let primary_mux_handle = muxer.add(audio::timewarp_stream::TimewarpStream::new(
            &core,
            supported_config.sample_rate(),
            supported_config.channels(),
        ));
        primary_mux_handle.switch();

        let audio_core_thread = mgba::thread::Thread::new(audio_core);
        audio_core_thread.start();
        audio_core_thread.handle().pause();
        audio_core_thread.handle().run_on_core(|mut core| {
            core.gba_mut()
                .sync_mut()
                .as_mut()
                .expect("sync")
                .set_fps_target(EXPECTED_FPS as f32);
        });

        let fastforwarder = fastforwarder::Fastforwarder::new(&rom_path, Box::new(bn6.clone()))?;

        core.set_traps(bn6.get_primary_traps(
            handle.clone(),
            facade::Facade::new(
                handle.clone(),
                match_state.clone(),
                joyflags.clone(),
                gui_state,
                config.clone(),
                audio_state_holder.clone(),
                audio_core_thread.handle(),
                primary_mux_handle,
                audio_core_mux_handle,
                Arc::new(parking_lot::Mutex::new(fastforwarder)),
            ),
        ));

        let thread = mgba::thread::Thread::new(core);
        thread.start();
        thread.handle().run_on_core(|mut core| {
            core.gba_mut()
                .sync_mut()
                .as_mut()
                .expect("sync")
                .set_fps_target(EXPECTED_FPS as f32);
        });
        {
            let joyflags = joyflags.clone();
            thread.set_frame_callback(move |mut core, video_buffer| {
                let mut vbuf = vbuf.lock();
                vbuf.copy_from_slice(video_buffer);
                for i in (0..vbuf.len()).step_by(4) {
                    vbuf[i + 3] = 0xff;
                }
                core.set_keys(joyflags.load(std::sync::atomic::Ordering::Relaxed));
                let mut emu_tps_counter = emu_tps_counter.lock();
                emu_tps_counter.mark();
            });
        }

        let stream = audio::open_stream(audio_device, &supported_config, muxer)?;
        stream.play()?;

        Ok(Loaded {
            match_state,
            joyflags,
            thread,
            _audio_core_thread: audio_core_thread,
            _stream: stream,
        })
    }

    pub fn thread_handle(&self) -> mgba::thread::Handle {
        self.thread.handle()
    }

    pub async fn lock_match_state(&self) -> tokio::sync::MutexGuard<'_, MatchState> {
        self.match_state.lock().await
    }

    pub fn set_joyflags(&self, joyflags: u32) {
        self.joyflags
            .store(joyflags, std::sync::atomic::Ordering::Relaxed)
    }
}
