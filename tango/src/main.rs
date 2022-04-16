#![windows_subsystem = "windows"]

const TANGO_CHILD_ENV_VAR: &str = "TANGO_CHILD";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    log::info!(
        "welcome to tango v{}-{}!",
        env!("CARGO_PKG_VERSION"),
        git_version::git_version!()
    );

    env_logger::Builder::from_default_env()
        .filter(Some("tango"), log::LevelFilter::Info)
        .init();
    if std::env::var(TANGO_CHILD_ENV_VAR).unwrap_or_default() == "1" {
        return child_main();
    }

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
        .args(
            std::env::args_os()
                .skip(1)
                .collect::<Vec<std::ffi::OsString>>(),
        )
        .env(TANGO_CHILD_ENV_VAR, "1")
        .stderr(log_file)
        .spawn()?
        .wait()?;

    Ok(())
}

fn child_main() -> Result<(), Box<dyn std::error::Error>> {
    mgba::log::init();
    let config = match tango::config::load() {
        Ok(config) => config,
        Err(e) => {
            log::warn!("failed to load config, will load default instead: {}", e);
            let config = tango::config::Config::default();
            tango::config::save(&config)?;
            config
        }
    };
    log::info!("current config: {:?}", config);
    let _ = std::fs::create_dir("roms");
    let _ = std::fs::create_dir("saves");
    let _ = std::fs::create_dir("replays");
    let g = tango::game::Game::new(config)?;
    g.run();
    Ok(())
}
