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
    core: Arc<Mutex<mgba::core::Core>>,
    match_state: Arc<tokio::sync::Mutex<MatchState>>,
    joyflags: Arc<std::sync::atomic::AtomicU32>,
    _trapper: mgba::trapper::Trapper,
    _audio_trapper: mgba::trapper::Trapper,
    _thread: mgba::thread::Thread,
    _audio_core_thread: mgba::thread::Thread,
    _stream: cpal::Stream,
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

        let core = Arc::new(Mutex::new({
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
            core
        }));

        let bn6 = {
            let core = core.clone();
            let core = core.lock();
            bn6::BN6::new(&core.as_ref().game_title()).unwrap()
        };

        let match_state = Arc::new(tokio::sync::Mutex::new(MatchState::NoMatch));

        let mut thread = {
            let mut thread = mgba::thread::Thread::new(core.clone());
            let mut core = core.lock();
            thread.start();
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
            let emu_tps_counter = emu_tps_counter;
            thread.set_frame_callback(Some(Box::new(move || {
                // TODO: This sometimes causes segfaults when the game gets unloaded.
                let core = core.lock();
                let mut vbuf = vbuf.lock();
                vbuf.copy_from_slice(core.video_buffer().unwrap());
                for i in (0..vbuf.len()).step_by(4) {
                    vbuf[i + 3] = 0xff;
                }
                let mut emu_tps_counter = emu_tps_counter.lock();
                emu_tps_counter.mark();
            })));
        }

        let joyflags = Arc::new(std::sync::atomic::AtomicU32::new(0));

        let audio_state_rendezvous = std::sync::Arc::new(parking_lot::Mutex::new(None));

        let trapper = {
            let core = core.clone();
            let mut core = core.lock();
            let fastforwarder =
                fastforwarder::Fastforwarder::new(&rom_path, Box::new(bn6.clone()))?;

            bn6.install_main_hooks(
                core.as_mut(),
                handle.clone(),
                facade::Facade::new(
                    handle.clone(),
                    match_state.clone(),
                    joyflags.clone(),
                    gui_state,
                    config.clone(),
                    audio_state_rendezvous.clone(),
                    Arc::new(parking_lot::Mutex::new(fastforwarder)),
                ),
            )
        };

        let audio_core = Arc::new(Mutex::new({
            let mut core = mgba::core::Core::new_gba("tango")?;
            let rom_vf = mgba::vfile::VFile::open(&rom_path, mgba::vfile::flags::O_RDONLY)?;
            core.as_mut().load_rom(rom_vf)?;
            core.as_mut().reset();
            core
        }));

        let audio_core_thread = {
            let mut audio_core_thread = mgba::thread::Thread::new(audio_core.clone());
            let mut audio_core = audio_core.lock();
            audio_core_thread.start();
            audio_core
                .as_mut()
                .gba_mut()
                .sync_mut()
                .as_mut()
                .expect("sync")
                .set_fps_target(60.0);
            audio_core_thread
        };

        let audio_trapper = {
            let audio_core = audio_core.clone();
            let mut audio_core = audio_core.lock();
            bn6.install_audio_hooks(audio_core.as_mut(), audio_state_rendezvous.clone())
        };

        let stream = audio::open_stream(
            audio_device,
            audio::mux_stream::MuxStream::new(vec![
                Box::new(audio::timewarp_stream::TimewarpStream::new(
                    audio_core.clone(),
                )),
                Box::new(audio::timewarp_stream::TimewarpStream::new(core.clone())),
            ]),
        )?;
        stream.play()?;

        Ok(Loaded {
            core,
            match_state,
            joyflags,
            _trapper: trapper,
            _audio_trapper: audio_trapper,
            _thread: thread,
            _audio_core_thread: audio_core_thread,
            _stream: stream,
        })
    }

    pub fn lock_core(&self) -> parking_lot::MutexGuard<mgba::core::Core> {
        self.core.lock()
    }

    pub async fn lock_match_state(&self) -> tokio::sync::MutexGuard<'_, MatchState> {
        self.match_state.lock().await
    }

    pub fn set_joyflags(&self, joyflags: u32) {
        self.joyflags
            .store(joyflags, std::sync::atomic::Ordering::Relaxed)
    }
}
