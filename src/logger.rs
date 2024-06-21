use crate::config::WorkingMode;

use anyhow::Result;
use log::LevelFilter;
use simplelog::{format_description, Config as LogConfig, ConfigBuilder};

#[cfg(debug_assertions)]
pub fn setup_logger(working_mode: WorkingMode) -> Result<()> {
    let is_serve = working_mode.is_serve();
    let config = build_config(is_serve);
    if is_serve {
        simplelog::SimpleLogger::init(LevelFilter::Debug, config)?;
    } else {
        let file = std::fs::File::create(crate::config::Config::local_path("debug.log")?)?;
        simplelog::WriteLogger::init(LevelFilter::Debug, config, file)?;
    }
    Ok(())
}

#[cfg(not(debug_assertions))]
pub fn setup_logger(working_mode: WorkingMode) -> Result<()> {
    if working_mode.is_serve() {
        let config = build_config(true);
        simplelog::SimpleLogger::init(log::LevelFilter::Info, config)?;
    }
    Ok(())
}

fn build_config(is_serve: bool) -> LogConfig {
    let log_filter = if is_serve {
        "aichat::serve".into()
    } else {
        match std::env::var("AICHAT_LOG_FILTER") {
            Ok(v) => v,
            Err(_) => "aichat".into(),
        }
    };
    ConfigBuilder::new()
        .add_filter_allow(log_filter)
        .set_time_format_custom(format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
        ))
        .set_thread_level(LevelFilter::Off)
        .build()
}
