use crate::{compat, config, current_input, gui, loaded, tps};
use cpal::traits::{DeviceTrait, HostTrait};
use parking_lot::Mutex;
use std::sync::Arc;

pub struct Game {
    rt: tokio::runtime::Runtime,
    compat_list: Arc<compat::CompatList>,
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
        let compat_list = std::sync::Arc::new(compat::load()?);

        log::info!(
            "wgpu adapters: {:?}",
            wgpu::Instance::new(wgpu::Backends::all())
                .enumerate_adapters(wgpu::Backends::all())
                .collect::<Vec<_>>()
        );
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
            let backends_str = &config.lock().graphics.backends;
            let wgpu_backends = if !backends_str.is_empty() {
                wgpu::util::parse_backends_from_comma_list(&backends_str)
            } else {
                wgpu::Backends::PRIMARY
            };
            let config = config.clone();
            let window_size = window.inner_size();
            let surface_texture =
                pixels::SurfaceTexture::new(window_size.width, window_size.height, &window);
            let pixels = pixels::PixelsBuilder::new(
                mgba::gba::SCREEN_WIDTH,
                mgba::gba::SCREEN_HEIGHT,
                surface_texture,
            )
            .wgpu_backend(wgpu_backends)
            .request_adapter_options(wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            })
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

                    let emu_tps_counter = emu_tps_counter.lock();
                    let fps_counter = fps_counter.lock();
                    let match_ = loaded.lock_match().await;
                    Some(gui::DebugStats {
                        fps: 1.0 / fps_counter.mean_duration().as_secs_f32(),
                        emu_tps: 1.0 / emu_tps_counter.mean_duration().as_secs_f32(),
                        match_: match &*match_ {
                            None => None,
                            Some(match_) => Some(gui::MatchDebugStats {
                                in_progress: match &*match_.lock_in_progress().await {
                                    Some(in_progress) => Some(gui::InProgressDebugStats {
                                        battle: {
                                            let battle_state =
                                                in_progress.lock_battle_state().await;
                                            match &battle_state.battle {
                                                Some(battle) => Some(gui::BattleDebugStats {
                                                    local_player_index: battle.local_player_index(),
                                                    local_qlen: battle.local_queue_length(),
                                                    remote_qlen: battle.remote_queue_length(),
                                                    local_delay: battle.local_delay(),
                                                    remote_delay: battle.remote_delay(),
                                                    tps_adjustment: battle.tps_adjustment(),
                                                }),
                                                None => None,
                                            }
                                        },
                                    }),
                                    None => None,
                                },
                            }),
                        },
                    })
                })
            })));
        };

        Ok(Game {
            rt,
            compat_list,
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

                let id = if let Some(id) = self
                    .compat_list
                    .id_by_title_and_crc32(&core.as_ref().game_title(), core.as_ref().crc32())
                {
                    id.to_string()
                } else {
                    log::warn!(
                        "could not find compatibility data for {} where title = {}, crc32 = {:08x}",
                        dirent.path().display(),
                        core.as_ref().game_title(),
                        core.as_ref().crc32()
                    );
                    return vec![];
                };

                vec![gui::ROMInfo {
                    path: dirent
                        .path()
                        .strip_prefix("roms")
                        .expect("strip prefix")
                        .to_owned(),
                    id,
                }]
            })
            .collect();
        rom_list.sort_unstable_by(|x, y| x.path.cmp(&y.path));

        let gui_state = self.gui.state();
        gui_state.set_rom_list(rom_list.clone());

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
                        } else {
                            // If an event was handled by the UI, clear our current input.
                            *current_input.borrow_mut() = current_input::CurrentInput::new();
                        }

                        let mut unfiltered_current_input = unfiltered_current_input.borrow_mut();
                        unfiltered_current_input.handle_event(window_event);
                    }
                    winit::event::Event::MainEventsCleared => {
                        {
                            let current_input = current_input.borrow();
                            let mut loaded = self.loaded.lock();

                            if let Some(loaded) = &*loaded {
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

                                loaded.set_joyflags(keys);
                            } else {
                                let gui_state = self.gui.state();

                                let selected_rom = {
                                    let mut selected_rom = None;
                                    let rom_select_state = gui_state.request_rom();
                                    match rom_select_state {
                                        gui::DialogState::Pending(_) => {}
                                        gui::DialogState::Ok(None) => {
                                            unreachable!();
                                        }
                                        gui::DialogState::Ok(Some(index)) => {
                                            selected_rom = Some(&rom_list[index]);
                                        }
                                        gui::DialogState::Closed => {}
                                    }
                                    selected_rom
                                };

                                if let Some(selected_rom) = selected_rom {
                                    log::info!("loading rom: {:?}", selected_rom);
                                    let save_filename = selected_rom.path.with_extension("sav");

                                    *loaded = Some(
                                        loaded::Loaded::new(
                                            &selected_rom.id,
                                            self.compat_list.clone(),
                                            &selected_rom.path,
                                            &save_filename,
                                            self.rt.handle().clone(),
                                            &self.audio_device,
                                            self.config.clone(),
                                            self.gui.state(),
                                            self.vbuf.clone(),
                                            self.emu_tps_counter.clone(),
                                        )
                                        .expect("loaded"),
                                    );
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

                        let mut current_input = current_input.borrow_mut();
                        current_input.step();

                        let mut unfiltered_current_input = unfiltered_current_input.borrow_mut();
                        unfiltered_current_input.step();
                    }
                    _ => {}
                }
            });
    }
}
