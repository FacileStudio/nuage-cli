use anyhow::{Context, Result};
use std::path::PathBuf;

pub fn nuage_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("cannot determine home directory")?;
    Ok(home.join(".nuage"))
}

pub fn pid_path() -> Result<PathBuf> {
    Ok(nuage_dir()?.join("nuage.pid"))
}

/// Path of the sidecar file describing the currently running daemon.
pub fn meta_path() -> Result<PathBuf> {
    Ok(nuage_dir()?.join("nuage.meta"))
}

pub fn log_dir() -> Result<PathBuf> {
    Ok(nuage_dir()?.join("logs"))
}

pub fn log_path() -> Result<PathBuf> {
    Ok(log_dir()?.join("nuage.log"))
}
