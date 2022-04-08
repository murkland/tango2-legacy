use crate::{audio, gui, mgba};

pub struct Game {
    main_core: std::sync::Arc<std::sync::Mutex<mgba::core::Core>>,
    event_loop: Option<winit::event_loop::EventLoop<()>>,
    input: winit_input_helper::WinitInputHelper,
    vbuf: std::sync::Arc<Vec<u8>>,
    vbuf2: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    window: winit::window::Window,
    pixels: pixels::Pixels,
    thread: mgba::thread::Thread,
    _stream: rodio::OutputStream,
    gui: gui::Gui,
}

impl Game {
    pub fn new() -> Result<Game, Box<dyn std::error::Error>> {
        let main_core = std::sync::Arc::new(std::sync::Mutex::new({
            let mut core = mgba::core::Core::new_gba("tango")?;
            core.set_audio_buffer_size(1024);

            let rom_vf = mgba::vfile::VFile::open("bn6f.gba", mgba::vfile::flags::O_RDONLY)?;
            core.load_rom(rom_vf);

            let save_vf = mgba::vfile::VFile::open(
                "bn6f.sav",
                mgba::vfile::flags::O_CREAT | mgba::vfile::flags::O_RDWR,
            )?;
            core.load_save(save_vf);

            log::info!("loaded game: {}", core.get_game_title());
            core
        }));

        let event_loop = Some(winit::event_loop::EventLoop::new());

        let (width, height, vbuf) = {
            let core = std::sync::Arc::clone(&main_core);
            let mut core = core.lock().unwrap();
            let (width, height) = core.desired_video_dimensions();
            let mut vbuf = vec![0u8; (width * height * 4) as usize];
            core.set_video_buffer(&mut vbuf, width.into());
            (width, height, std::sync::Arc::new(vbuf))
        };

        let input = winit_input_helper::WinitInputHelper::new();

        let window = {
            let size = winit::dpi::LogicalSize::new(width * 3, height * 3);
            winit::window::WindowBuilder::new()
                .with_title("tango")
                .with_inner_size(size)
                .with_min_inner_size(size)
                .build(event_loop.as_ref().unwrap())?
        };

        let vbuf2 = std::sync::Arc::new(std::sync::Mutex::new(vec![
            0u8;
            (width * height * 4) as usize
        ]));

        let pixels = {
            let window_size = window.inner_size();
            let surface_texture =
                pixels::SurfaceTexture::new(window_size.width, window_size.height, &window);
            pixels::PixelsBuilder::new(width, height, surface_texture)
                .request_adapter_options(pixels::wgpu::RequestAdapterOptions {
                    power_preference: pixels::wgpu::PowerPreference::HighPerformance,
                    force_fallback_adapter: false,
                    compatible_surface: None,
                })
                .build()?
        };

        let mut thread = {
            let core = std::sync::Arc::clone(&main_core);
            mgba::thread::Thread::new(core)
        };
        thread.start();

        let (stream, stream_handle) = rodio::OutputStream::try_default().unwrap();
        let audio_source = {
            let core = std::sync::Arc::clone(&main_core);
            audio::MGBAAudioSource::new(core, 48000)
        };
        stream_handle.play_raw(audio_source)?;

        {
            let core = std::sync::Arc::clone(&main_core);
            let mut core = core.lock().unwrap();
            core.get_gba_mut()
                .get_sync_mut()
                .as_mut()
                .unwrap()
                .set_fps_target(60.0);
        }

        let gui = gui::Gui::new(&window, &pixels);

        let mut game = Game {
            main_core,
            event_loop,
            input,
            window,
            pixels,
            vbuf,
            vbuf2,
            thread,
            _stream: stream,
            gui,
        };

        {
            let vbuf = std::sync::Arc::clone(&game.vbuf);
            let vbuf2 = std::sync::Arc::clone(&game.vbuf2);
            game.thread.set_frame_callback(Some(Box::new(move || {
                let mut vbuf2 = vbuf2.lock().unwrap();
                vbuf2.copy_from_slice(&vbuf);
                for i in (0..vbuf2.len()).step_by(4) {
                    vbuf2[i + 3] = 0xff;
                }
            })));
        }

        Ok(game)
    }

    pub fn run(mut self: Self) {
        self.event_loop
            .take()
            .unwrap()
            .run(move |event, _, control_flow| {
                *control_flow = winit::event_loop::ControlFlow::Poll;

                if let winit::event::Event::RedrawRequested(_) = event {
                    {
                        let vbuf2 = self.vbuf2.lock().unwrap().clone();
                        self.pixels.get_frame().copy_from_slice(&vbuf2);
                    }

                    self.gui
                        .prepare(&self.window)
                        .expect("gui.prepare() failed");
                    self.pixels
                        .render_with(|encoder, render_target, context| {
                            context.scaling_renderer.render(encoder, render_target);
                            self.gui
                                .render(&self.window, encoder, render_target, context)?;
                            Ok(())
                        })
                        .unwrap();
                }

                self.gui.handle_event(&self.window, &event);
                if self.input.update(&event) {
                    if self.input.quit() {
                        *control_flow = winit::event_loop::ControlFlow::Exit;
                        return;
                    }

                    if let Some(size) = self.input.window_resized() {
                        self.pixels.resize_surface(size.width, size.height);
                    }

                    let mut core = self.main_core.lock().unwrap();

                    let mut keys = 0u32;
                    if self.input.key_held(winit::event::VirtualKeyCode::Left) {
                        keys |= mgba::input::keys::LEFT;
                    }
                    if self.input.key_held(winit::event::VirtualKeyCode::Right) {
                        keys |= mgba::input::keys::RIGHT;
                    }
                    if self.input.key_held(winit::event::VirtualKeyCode::Up) {
                        keys |= mgba::input::keys::UP;
                    }
                    if self.input.key_held(winit::event::VirtualKeyCode::Down) {
                        keys |= mgba::input::keys::DOWN;
                    }
                    if self.input.key_held(winit::event::VirtualKeyCode::Z) {
                        keys |= mgba::input::keys::A;
                    }
                    if self.input.key_held(winit::event::VirtualKeyCode::X) {
                        keys |= mgba::input::keys::B;
                    }
                    if self.input.key_held(winit::event::VirtualKeyCode::A) {
                        keys |= mgba::input::keys::L;
                    }
                    if self.input.key_held(winit::event::VirtualKeyCode::S) {
                        keys |= mgba::input::keys::R;
                    }
                    if self.input.key_held(winit::event::VirtualKeyCode::Return) {
                        keys |= mgba::input::keys::START;
                    }
                    if self.input.key_held(winit::event::VirtualKeyCode::Back) {
                        keys |= mgba::input::keys::SELECT;
                    }

                    core.set_keys(keys);

                    self.window.request_redraw();
                }
            });
    }
}
