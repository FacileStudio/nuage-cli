use anyhow::{Context, Result};
use std::path::PathBuf;

pub fn nuage_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("cannot determine home directory")?;
    Ok(home.join(".nuage"))
}

pub fn pid_path() -> Result<PathBuf> {
    Ok(nuage_dir()?.join("nuage.pid"))
}

pub fn log_dir() -> Result<PathBuf> {
    Ok(nuage_dir()?.join("logs"))
}

pub fn log_path() -> Result<PathBuf> {
    Ok(log_dir()?.join("nuage.log"))
}

pub fn is_running() -> Result<Option<u32>> {
    let path = pid_path()?;
    if !path.exists() {
        return Ok(None);
    }

    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => {
            let _ = std::fs::remove_file(&path);
            return Ok(None);
        }
    };

    let pid: u32 = match contents.trim().parse() {
        Ok(p) if p > 0 => p,
        _ => {
            let _ = std::fs::remove_file(&path);
            return Ok(None);
        }
    };

    let alive = unsafe { libc::kill(pid as i32, 0) == 0 };
    if alive {
        Ok(Some(pid))
    } else {
        let _ = std::fs::remove_file(&path);
        Ok(None)
    }
}

pub fn init_daemon_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .with_ansi(false)
        .with_target(false)
        .with_timer(tracing_subscriber::fmt::time::time())
        .init();
}

pub fn init_terminal_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .with_target(false)
        .with_timer(tracing_subscriber::fmt::time::time())
        .init();
}
