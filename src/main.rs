#[macro_use]
extern crate lazy_static;

mod mgba;

fn main() {
    let mut core = mgba::Core::new_gba().unwrap();
    let rom_vf = mgba::VFile::open("bn6f.gba", 0).unwrap();
    core.load_rom(rom_vf);
    core.reset();
    mgba::set_default_logger(&|| {});

    let now = std::time::Instant::now();
    for i in 1..1000 {
        core.run_frame();
    }
    println!("{:?}", now.elapsed());
}
