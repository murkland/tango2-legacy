#[macro_use]
extern crate lazy_static;

mod mgba;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let mut core = mgba::core::Core::new_gba("tango").unwrap();
    let rom_vf = mgba::vfile::VFile::open("bn6f.gba", 0).unwrap();
    core.load_rom(rom_vf);
    core.reset();
    mgba::log::set_default_logger(Box::new(&|category, level, message| {
        log::info!("{}", message)
    }));

    let (width, height) = core.desired_video_dimensions();

    let event_loop = winit::event_loop::EventLoop::new();
    let mut input = winit_input_helper::WinitInputHelper::new();

    let window = winit::window::WindowBuilder::new()
        .with_title("tango")
        .with_inner_size(winit::dpi::LogicalSize::new(width, height))
        .build(&event_loop)
        .unwrap();

    let mut pixels = {
        let window_size = window.inner_size();
        let surface_texture =
            pixels::SurfaceTexture::new(window_size.width, window_size.height, &window);
        pixels::Pixels::new(width, height, surface_texture)?
    };

    event_loop.run(move |event, _, control_flow| {
        if let winit::event::Event::RedrawRequested(_) = event {
            if pixels
                .render()
                .map_err(|e| log::error!("pixels.render() failed: {}", e))
                .is_err()
            {
                *control_flow = winit::event_loop::ControlFlow::Exit;
                return;
            }
        }

        if input.update(&event) {
            if input.quit() {
                *control_flow = winit::event_loop::ControlFlow::Exit;
                return;
            }
        }
        window.request_redraw();
    });
}
