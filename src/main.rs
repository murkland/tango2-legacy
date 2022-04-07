#[macro_use]
extern crate lazy_static;

mod mgba;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    mgba::log::set_default_logger(Box::new(&|category, level, message| {
        log::info!("{}", message)
    }));

    let core = std::sync::Arc::new(std::sync::Mutex::new(
        mgba::core::Core::new_gba("tango").unwrap(),
    ));

    let (width, height, vbuf) = {
        let core = std::sync::Arc::clone(&core);
        let mut core = core.lock().unwrap();
        let rom_vf = mgba::vfile::VFile::open("bn6f.gba", 0).unwrap();
        core.load_rom(rom_vf);

        let (width, height) = core.desired_video_dimensions();
        let mut vbuf = vec![0u8; (width * height * 4) as usize];
        core.set_video_buffer(&mut vbuf, width.into());
        (width, height, vbuf)
    };

    let event_loop = winit::event_loop::EventLoop::new();
    let mut input = winit_input_helper::WinitInputHelper::new();

    let window = {
        let size = winit::dpi::LogicalSize::new(width * 3, height * 3);
        winit::window::WindowBuilder::new()
            .with_title("tango")
            .with_inner_size(size)
            .with_min_inner_size(size)
            .build(&event_loop)
            .unwrap()
    };

    let pixels = std::sync::Arc::new(std::sync::Mutex::new({
        let window_size = window.inner_size();
        let surface_texture =
            pixels::SurfaceTexture::new(window_size.width, window_size.height, &window);
        pixels::Pixels::new(width, height, surface_texture)?
    }));

    let mut thread = {
        let core = std::sync::Arc::clone(&core);
        mgba::thread::Thread::new(core)
    };

    {
        let pixels = std::sync::Arc::clone(&pixels);
        thread.frame_callback = Some(Box::new(move || {
            let mut pixels = pixels.lock().unwrap();
            let frame = pixels.get_frame();
            frame.copy_from_slice(&vbuf);
            for i in (0..frame.len()).step_by(4) {
                frame[i + 3] = 0xff;
            }
        }));
    }
    thread.start();

    event_loop.run(move |event, _, control_flow| {
        *control_flow = winit::event_loop::ControlFlow::Poll;

        if let winit::event::Event::MainEventsCleared = event {
            pixels.lock().unwrap().render().unwrap();
        }

        if input.update(&event) {
            if input.quit() {
                *control_flow = winit::event_loop::ControlFlow::Exit;
                return;
            }

            if let Some(size) = input.window_resized() {
                println!(":V");
                pixels
                    .lock()
                    .unwrap()
                    .resize_surface(size.width, size.height);
            }
        }
    });
}
