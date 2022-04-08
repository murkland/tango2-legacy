#[macro_use]
extern crate lazy_static;

mod audio;
mod bn6;
mod fastforwarder;
mod game;
mod gui;
mod input;
mod mgba;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    mgba::log::init();

    let g = game::Game::new()?;
    g.run();
    Ok(())
}
