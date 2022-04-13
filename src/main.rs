#![windows_subsystem = "windows"]

#[macro_use]
extern crate lazy_static;

mod audio;
mod battle;
mod bn6;
mod config;
mod current_input;
mod datachannel;
mod fastforwarder;
mod game;
mod gui;
mod input;
mod loaded;
mod locales;
mod mgba;
mod protocol;
mod replay;
mod tps;

const TANGO_CHILD_ENV_VAR: &str = "TANGO_CHILD";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_default_env()
        .filter(Some("tango"), log::LevelFilter::Info)
        .init();
    if std::env::var(TANGO_CHILD_ENV_VAR).unwrap_or_default() == "true" {
        return child_main();
    }

    log::info!(
        "welcome to tango v{}-{}!",
        env!("CARGO_PKG_VERSION"),
        git_version::git_version!()
    );

    let log_filename = format!(
        "{}.log",
        time::OffsetDateTime::from(std::time::SystemTime::now())
            .format(time::macros::format_description!(
                "[year padding:zero][month padding:zero repr:numerical][day padding:zero][hour padding:zero][minute padding:zero][second padding:zero]"
            ))
            .expect("format time"),
    );

    let _ = std::fs::create_dir("logs");
    let log_path = std::path::Path::new("logs").join(log_filename);
    log::info!("logging to: {}", log_path.display());

    let log_file = std::fs::File::create(log_path)?;

    std::process::Command::new(std::env::current_exe()?)
        .args(std::env::args())
        .env(TANGO_CHILD_ENV_VAR, "true")
        .stderr(log_file)
        .spawn()?
        .wait()?;

    Ok(())
}

fn child_main() -> Result<(), Box<dyn std::error::Error>> {
    mgba::log::init();
    let config = match config::load_config() {
        Ok(config) => config,
        Err(e) => {
            log::warn!("failed to load config, will load default instead: {}", e);
            let config = config::Config::default();
            config::save_config(&config)?;
            config
        }
    };
    log::info!("current config: {:?}", config);
    let _ = std::fs::create_dir("roms");
    let _ = std::fs::create_dir("saves");
    let _ = std::fs::create_dir("replays");
    let g = game::Game::new(config)?;
    g.run();
    Ok(())
}
