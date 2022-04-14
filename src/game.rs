use crate::{bn6, config, current_input, gui, loaded, tps};
use cpal::traits::{DeviceTrait, HostTrait};
use parking_lot::Mutex;
use std::sync::Arc;

pub struct Game {
    rt: tokio::runtime::Runtime,
    fps_counter: Arc<Mutex<tps::Counter>>,
    event_loop: Option<winit::event_loop::EventLoop<()>>,
    audio_device: cpal::Device,
    window: winit::window::Window,
    pixels: pixels::Pixels,
    gui: gui::Gui,
    config: Arc<Mutex<config::Config>>,
    vbuf: Arc<Mutex<Vec<u8>>>,
    current_input: std::rc::Rc<std::cell::RefCell<current_input::CurrentInput>>,
    unfiltered_current_input: std::rc::Rc<std::cell::RefCell<current_input::CurrentInput>>,
    loaded: Arc<Mutex<Option<loaded::Loaded>>>,
    emu_tps_counter: Arc<Mutex<tps::Counter>>,
}

impl Game {
    pub fn new(config: config::Config) -> Result<Game, anyhow::Error> {
        let audio_device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| anyhow::format_err!("could not open audio device"))?;
        log::info!(
            "supported audio output configs: {:?}",
            audio_device.supported_output_configs()?.collect::<Vec<_>>()
        );

        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;

        {
            let bind_addr = config.matchmaking.bind_addr.to_string();
            rt.spawn(async {
                let bind_addr2 = bind_addr.clone();
                if let Err(e) = (move || async {
                    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
                    log::info!("bound local matchmaking server on {}", listener.local_addr()?);
                    let mut server = tango_matchmaking::server::Server::new(listener);
                    server.run().await;
                    Result::<(), anyhow::Error>::Ok(())
                })()
                .await
                {
                    log::info!("failed to bind local matchmaking server to {}, direct connect will not be available: {}", bind_addr2, e)
                }
            });
        }

        let handle = rt.handle().clone();

        let event_loop = Some(winit::event_loop::EventLoop::new());

        let current_input =
            std::rc::Rc::new(std::cell::RefCell::new(current_input::CurrentInput::new()));
        let unfiltered_current_input =
            std::rc::Rc::new(std::cell::RefCell::new(current_input::CurrentInput::new()));

        let vbuf = Arc::new(Mutex::new(vec![
            0u8;
            (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 4)
                as usize
        ]));

        let window = {
            let size = winit::dpi::LogicalSize::new(
                mgba::gba::SCREEN_WIDTH * 3,
                mgba::gba::SCREEN_HEIGHT * 3,
            );
            winit::window::WindowBuilder::new()
                .with_title("tango")
                .with_inner_size(size)
                .with_min_inner_size(size)
                .build(event_loop.as_ref().expect("event loop"))?
        };

        let config = Arc::new(Mutex::new(config));

        let fps_counter = Arc::new(Mutex::new(tps::Counter::new(30)));
        let emu_tps_counter = Arc::new(Mutex::new(tps::Counter::new(10)));

        let (pixels, gui) = {
            let config = config.clone();
            let window_size = window.inner_size();
            let surface_texture =
                pixels::SurfaceTexture::new(window_size.width, window_size.height, &window);
            let pixels = pixels::PixelsBuilder::new(
                mgba::gba::SCREEN_WIDTH,
                mgba::gba::SCREEN_HEIGHT,
                surface_texture,
            )
            .build()?;
            let gui = gui::Gui::new(
                config,
                unfiltered_current_input.clone(),
                window_size.width,
                window_size.height,
                window.scale_factor() as f32,
                &pixels,
            );
            (pixels, gui)
        };

        let gui_state = gui.state();

        let loaded = Arc::new(Mutex::new(None::<loaded::Loaded>));

        {
            let loaded = Arc::downgrade(&loaded);
            let fps_counter = fps_counter.clone();
            let emu_tps_counter = emu_tps_counter.clone();
            let handle = handle;
            gui_state.set_debug_stats_getter(Some(Box::new(move || {
                handle.block_on(async {
                    let loaded = loaded.upgrade()?;
                    let loaded = loaded.lock();
                    let loaded = if let Some(loaded) = &*loaded {
                        loaded
                    } else {
                        return None;
                    };

                    let core = loaded.lock_core();
                    let emu_tps_counter = emu_tps_counter.lock();
                    let fps_counter = fps_counter.lock();
                    let match_state = loaded.lock_match_state().await;
                    Some(gui::DebugStats {
                        fps: 1.0 / fps_counter.mean_duration().as_secs_f32(),
                        emu_tps: 1.0 / emu_tps_counter.mean_duration().as_secs_f32(),
                        target_tps: core.as_ref().gba().sync().unwrap().fps_target(),
                        match_state: match &*match_state {
                            loaded::MatchState::NoMatch => "none",
                            loaded::MatchState::Aborted => "aborted",
                            loaded::MatchState::Match(_) => "active",
                        },
                        battle_debug_stats: match &*match_state {
                            loaded::MatchState::NoMatch => None,
                            loaded::MatchState::Aborted => None,
                            loaded::MatchState::Match(m) => {
                                let battle_state = m.lock_battle_state().await;
                                match &battle_state.battle {
                                    Some(battle) => Some(gui::BattleDebugStats {
                                        local_player_index: battle.local_player_index(),
                                        local_qlen: battle.local_queue_length().await,
                                        remote_qlen: battle.remote_queue_length().await,
                                        local_delay: battle.local_delay(),
                                        remote_delay: battle.remote_delay(),
                                    }),
                                    None => None,
                                }
                            }
                        },
                    })
                })
            })));
        };

        Ok(Game {
            rt,
            audio_device,
            config,
            fps_counter,
            current_input,
            unfiltered_current_input,
            event_loop,
            window,
            pixels,
            vbuf,
            gui,
            loaded,
            emu_tps_counter,
        })
    }

    pub fn load(&self, rom_filename: &std::path::Path) -> anyhow::Result<()> {
        let save_filename = rom_filename.with_extension("sav");

        *self.loaded.lock() = {
            let handle = self.rt.handle().clone();
            Some(loaded::Loaded::new(
                rom_filename,
                &save_filename,
                handle,
                &self.audio_device,
                self.config.clone(),
                self.gui.state(),
                self.vbuf.clone(),
                self.emu_tps_counter.clone(),
            )?)
        };
        Ok(())
    }

    pub fn run(mut self) {
        let mut rom_list: Vec<gui::ROMInfo> = std::fs::read_dir("roms")
            .expect("roms")
            .flat_map(|dirent| {
                let dirent = dirent.expect("dirent");
                let mut core = mgba::core::Core::new_gba("tango").expect("new_gba");
                let vf =
                    match mgba::vfile::VFile::open(&dirent.path(), mgba::vfile::flags::O_RDONLY) {
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

                let title = core.as_ref().game_title();
                if bn6::BN6::new(&title).is_none() {
                    return vec![];
                }

                vec![gui::ROMInfo {
                    path: dirent
                        .path()
                        .strip_prefix("roms")
                        .expect("strip prefix")
                        .to_owned(),
                    title,
                }]
            })
            .collect();
        rom_list.sort_unstable_by(|x, y| x.path.cmp(&y.path));

        // Probe for ROMs.
        {
            let gui_state = self.gui.state();
            gui_state.set_rom_list(rom_list.clone());
            gui_state.open_rom_select_dialog();
        }

        let current_input = self.current_input.clone();
        let unfiltered_current_input = self.unfiltered_current_input.clone();

        self.event_loop
            .take()
            .expect("event loop")
            .run(move |event, _, control_flow| {
                *control_flow = winit::event_loop::ControlFlow::Poll;

                match event {
                    winit::event::Event::WindowEvent {
                        event: ref window_event,
                        ..
                    } => {
                        match window_event {
                            winit::event::WindowEvent::CloseRequested => {
                                *control_flow = winit::event_loop::ControlFlow::Exit;
                            }
                            winit::event::WindowEvent::Resized(size) => {
                                self.pixels.resize_surface(size.width, size.height);
                                self.gui.resize(size.width, size.height);
                            }
                            _ => {}
                        };
                        if !self.gui.handle_event(window_event) {
                            let mut current_input = current_input.borrow_mut();
                            current_input.handle_event(window_event);
                        }

                        let mut unfiltered_current_input = unfiltered_current_input.borrow_mut();
                        unfiltered_current_input.handle_event(window_event);
                    }
                    winit::event::Event::MainEventsCleared => {
                        {
                            let current_input = current_input.borrow();

                            if let Some(loaded) = &*self.loaded.lock() {
                                let mut core = loaded.lock_core();
                                let config = self.config.lock();

                                let mut keys = 0u32;
                                if current_input.key_held[config.keymapping.left as usize] {
                                    keys |= mgba::input::keys::LEFT;
                                }
                                if current_input.key_held[config.keymapping.right as usize] {
                                    keys |= mgba::input::keys::RIGHT;
                                }
                                if current_input.key_held[config.keymapping.up as usize] {
                                    keys |= mgba::input::keys::UP;
                                }
                                if current_input.key_held[config.keymapping.down as usize] {
                                    keys |= mgba::input::keys::DOWN;
                                }
                                if current_input.key_held[config.keymapping.a as usize] {
                                    keys |= mgba::input::keys::A;
                                }
                                if current_input.key_held[config.keymapping.b as usize] {
                                    keys |= mgba::input::keys::B;
                                }
                                if current_input.key_held[config.keymapping.l as usize] {
                                    keys |= mgba::input::keys::L;
                                }
                                if current_input.key_held[config.keymapping.r as usize] {
                                    keys |= mgba::input::keys::R;
                                }
                                if current_input.key_held[config.keymapping.start as usize] {
                                    keys |= mgba::input::keys::START;
                                }
                                if current_input.key_held[config.keymapping.select as usize] {
                                    keys |= mgba::input::keys::SELECT;
                                }

                                core.as_mut().set_keys(keys);
                                loaded.set_joyflags(keys);
                            }

                            {
                                let gui_state = self.gui.state();

                                let selected_rom = {
                                    let mut selected_rom = None;
                                    let rom_select_state = gui_state.lock_rom_select_state();
                                    match &*rom_select_state {
                                        gui::DialogState::Pending(_) => {}
                                        gui::DialogState::Ok(None) => {
                                            unreachable!();
                                        }
                                        gui::DialogState::Ok(Some(index)) => {
                                            selected_rom = Some(&rom_list[*index]);
                                        }
                                        gui::DialogState::Cancelled => {
                                            unreachable!();
                                        }
                                        gui::DialogState::Closed => {}
                                    }
                                    selected_rom
                                };

                                if let Some(selected_rom) = selected_rom {
                                    log::info!("loading rom: {:?}", selected_rom);
                                    self.load(&selected_rom.path).expect("load rom");
                                    gui_state.close_rom_select_dialog();
                                }
                            }

                            let unfiltered_current_input = unfiltered_current_input.borrow();
                            if unfiltered_current_input.key_actions.iter().any(|action| {
                                matches!(
                                    action,
                                    current_input::KeyAction::Pressed(
                                        winit::event::VirtualKeyCode::Escape,
                                    )
                                )
                            }) {
                                self.gui.state().toggle_menu();
                            }
                        }

                        let vbuf = self.vbuf.lock().clone();
                        self.pixels.get_frame().copy_from_slice(&vbuf);

                        self.gui.prepare(&self.window);
                        self.pixels
                            .render_with(|encoder, render_target, context| {
                                context.scaling_renderer.render(encoder, render_target);
                                self.gui.render(encoder, render_target, context)?;
                                Ok(())
                            })
                            .expect("render pixels");
                        self.fps_counter.lock().mark();

                        {
                            let mut current_input = current_input.borrow_mut();
                            current_input.step();
                        }

                        {
                            let mut unfiltered_current_input =
                                unfiltered_current_input.borrow_mut();
                            unfiltered_current_input.step();
                        }
                    }
                    _ => {}
                }
            });
    }
}
