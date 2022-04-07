#[macro_use]
extern crate lazy_static;

mod audio;
mod gui;
mod mgba;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    mgba::log::init();

    let core = std::sync::Arc::new(std::sync::Mutex::new({
        let mut core = mgba::core::Core::new_gba("tango").unwrap();
        core.set_audio_buffer_size(1024);

        let rom_vf = mgba::vfile::VFile::open("bn6f.gba", 0).unwrap();
        core.load_rom(rom_vf);

        log::info!("loaded game: {}", core.get_game_title());
        core
    }));

    let mut trapper = {
        let core = std::sync::Arc::clone(&core);
        let trapper = mgba::trapper::Trapper::new(core);
        trapper
    };
    trapper.attach();

    let (width, height, vbuf) = {
        let core = std::sync::Arc::clone(&core);
        let mut core = core.lock().unwrap();
        let (width, height) = core.desired_video_dimensions();
        let mut vbuf = vec![0u8; (width * height * 4) as usize];
        core.set_video_buffer(&mut vbuf, width.into());
        (width, height, vbuf)
    };

    let vbuf2 = std::sync::Arc::new(std::sync::Mutex::new(vec![
        0u8;
        (width * height * 4) as usize
    ]));

    let event_loop = winit::event_loop::EventLoop::new();
    let mut input = winit_input_helper::WinitInputHelper::new();

    let window = {
        let size = winit::dpi::LogicalSize::new(width * 3, height * 3);
        winit::window::WindowBuilder::new()
            .with_title("tango")
            .with_inner_size(size)
            .with_min_inner_size(size)
            .build(&event_loop)?
    };

    let mut pixels = {
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
        let core = std::sync::Arc::clone(&core);
        mgba::thread::Thread::new(core)
    };

    let (_stream, stream_handle) = rodio::OutputStream::try_default().unwrap();
    let audio_source = {
        let core = std::sync::Arc::clone(&core);
        audio::MGBAAudioSource::new(core, 48000)
    };
    stream_handle.play_raw(audio_source)?;

    {
        let vbuf2 = std::sync::Arc::clone(&vbuf2);
        thread.frame_callback = Some(Box::new(move || {
            let mut vbuf2 = vbuf2.lock().unwrap();
            vbuf2.copy_from_slice(&vbuf);
            for i in (0..vbuf2.len()).step_by(4) {
                vbuf2[i + 3] = 0xff;
            }
        }));
    }
    thread.start();
    {
        let core = std::sync::Arc::clone(&core);
        let mut core = core.lock().unwrap();
        core.get_gba().get_sync().unwrap().set_fps_target(60.0);
    }

    let mut gui = gui::Gui::new(&window, &pixels);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = winit::event_loop::ControlFlow::Poll;

        if let winit::event::Event::RedrawRequested(_) = event {
            {
                let vbuf2 = vbuf2.lock().unwrap().clone();
                pixels.get_frame().copy_from_slice(&vbuf2);
            }

            gui.prepare(&window).expect("gui.prepare() failed");
            pixels
                .render_with(|encoder, render_target, context| {
                    context.scaling_renderer.render(encoder, render_target);
                    gui.render(&window, encoder, render_target, context)?;
                    Ok(())
                })
                .unwrap();
        }

        gui.handle_event(&window, &event);
        if input.update(&event) {
            if input.quit() {
                *control_flow = winit::event_loop::ControlFlow::Exit;
                return;
            }

            if let Some(size) = input.window_resized() {
                pixels.resize_surface(size.width, size.height);
            }

            let mut core = core.lock().unwrap();

            let mut keys = 0u32;
            if input.key_held(winit::event::VirtualKeyCode::Left) {
                keys |= mgba::input::keys::LEFT;
            }
            if input.key_held(winit::event::VirtualKeyCode::Right) {
                keys |= mgba::input::keys::RIGHT;
            }
            if input.key_held(winit::event::VirtualKeyCode::Up) {
                keys |= mgba::input::keys::UP;
            }
            if input.key_held(winit::event::VirtualKeyCode::Down) {
                keys |= mgba::input::keys::DOWN;
            }
            if input.key_held(winit::event::VirtualKeyCode::Z) {
                keys |= mgba::input::keys::A;
            }
            if input.key_held(winit::event::VirtualKeyCode::X) {
                keys |= mgba::input::keys::B;
            }
            if input.key_held(winit::event::VirtualKeyCode::A) {
                keys |= mgba::input::keys::L;
            }
            if input.key_held(winit::event::VirtualKeyCode::S) {
                keys |= mgba::input::keys::R;
            }
            if input.key_held(winit::event::VirtualKeyCode::Return) {
                keys |= mgba::input::keys::START;
            }
            if input.key_held(winit::event::VirtualKeyCode::Back) {
                keys |= mgba::input::keys::SELECT;
            }

            core.set_keys(keys);

            window.request_redraw();
        }
    });
}
