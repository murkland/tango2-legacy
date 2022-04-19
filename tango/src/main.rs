#![windows_subsystem = "windows"]

const TANGO_CHILD_ENV_VAR: &str = "TANGO_CHILD";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = match tango::config::load() {
        Ok(config) => config,
        Err(e) => {
            log::warn!("failed to load config, will load default instead: {}", e);
            let config = tango::config::Config::default();
            tango::config::save(&config)?;
            config
        }
    };

    env_logger::Builder::from_default_env()
        .filter(Some("tango"), log::LevelFilter::Info)
        .parse_filters(&config.logging.filters)
        .init();

    log::info!(
        "welcome to tango v{}-{}!",
        env!("CARGO_PKG_VERSION"),
        git_version::git_version!()
    );
    log::info!("current config: {:?}", config);

    if std::env::var(TANGO_CHILD_ENV_VAR).unwrap_or_default() == "1" {
        return child_main(config);
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

    let status = std::process::Command::new(std::env::current_exe()?)
        .args(
            std::env::args_os()
                .skip(1)
                .collect::<Vec<std::ffi::OsString>>(),
        )
        .env(TANGO_CHILD_ENV_VAR, "1")
        .stderr(log_file)
        .spawn()?
        .wait()?;

    if let Some(code) = status.code() {
        std::process::exit(code);
    }

    Ok(())
}

fn child_main(config: tango::config::Config) -> Result<(), Box<dyn std::error::Error>> {
    mgba::log::init();
    let _ = std::fs::create_dir("roms");
    let _ = std::fs::create_dir("saves");
    let _ = std::fs::create_dir("replays");
    let g = tango::game::Game::new(config)?;
    g.run();
    Ok(())
}
