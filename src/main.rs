#[macro_use]
extern crate lazy_static;

mod mgba;

fn main() {
    let mut core = mgba::core::Core::new_gba("tango").unwrap();
    let rom_vf = mgba::vfile::VFile::open("bn6f.gba", 0).unwrap();
    core.load_rom(rom_vf);
    core.reset();
    mgba::log::set_default_logger(Box::new(&|category, level, message| print!("{}", message)));

    let now = std::time::Instant::now();
    for i in 1..1000 {
        core.run_frame();
    }
    println!("{:?}", now.elapsed());
}
