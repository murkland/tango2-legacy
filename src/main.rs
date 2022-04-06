#[macro_use]
extern crate lazy_static;

mod mgba;

struct State {}

impl State {
    fn new() -> Self {
        State {}
    }
}

impl ggez::event::EventHandler<ggez::GameError> for State {
    fn update(&mut self, _ctx: &mut ggez::Context) -> ggez::GameResult {
        Ok(())
    }

    fn draw(&mut self, ctx: &mut ggez::Context) -> ggez::GameResult {
        Ok(())
    }
}

fn main() -> ggez::GameResult {
    let mut core = mgba::core::Core::new_gba("tango").unwrap();
    let rom_vf = mgba::vfile::VFile::open("bn6f.gba", 0).unwrap();
    core.load_rom(rom_vf);
    core.reset();
    mgba::log::set_default_logger(Box::new(&|category, level, message| print!("{}", message)));

    let cb = ggez::ContextBuilder::new("tango", "the tango authors")
        .window_setup(ggez::conf::WindowSetup::default().title("tango"));
    let (ctx, event_loop) = cb.build()?;
    let state = State::new();
    ggez::event::run(ctx, event_loop, state)
}
