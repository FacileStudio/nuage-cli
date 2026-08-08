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

/// Metadata about the daemon process recorded next to the pid file.
#[derive(Debug, Clone)]
pub struct DaemonMeta {
    pub started_at: String,
    pub exe: String,
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
    if !alive {
        let _ = clear_runtime_files();
        return Ok(None);
    }

    match is_nuage_process(pid) {
        Some(true) | None => Ok(Some(pid)),
        Some(false) => {
            let _ = clear_runtime_files();
            Ok(None)
        }
    }
}

fn is_nuage_process(pid: u32) -> Option<bool> {
    let output = std::process::Command::new("ps")
        .arg("-p")
        .arg(pid.to_string())
        .arg("-o")
        .arg("comm=")
        .output()
        .ok()?;

    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if name.is_empty() {
        if output.status.success() {
            return None;
        }
        return Some(false);
    }

    let base = name.rsplit('/').next().unwrap_or(&name);
    Some(base.starts_with("nuage"))
}

/// Writes the daemon start timestamp and executable path to `~/.nuage/nuage.meta`.
pub fn write_meta() -> Result<()> {
    let dir = nuage_dir()?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("cannot create {}", dir.display()))?;

    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let started_at = chrono::Utc::now().to_rfc3339();
    let contents = format!("started_at={}\nexe={}\n", started_at, exe);

    let path = meta_path()?;
    std::fs::write(&path, contents)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

/// Reads `~/.nuage/nuage.meta`; a missing or malformed file yields `Ok(None)`.
pub fn read_meta() -> Result<Option<DaemonMeta>> {
    let path = meta_path()?;
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };

    let mut started_at = None;
    let mut exe = None;
    for line in contents.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "started_at" => started_at = Some(value.trim().to_string()),
            "exe" => exe = Some(value.trim().to_string()),
            _ => {}
        }
    }

    match (started_at, exe) {
        (Some(started_at), Some(exe)) if !started_at.is_empty() => {
            Ok(Some(DaemonMeta { started_at, exe }))
        }
        _ => Ok(None),
    }
}

/// Removes the pid and meta files, ignoring files that are already gone.
pub fn clear_runtime_files() -> Result<()> {
    for path in [pid_path()?, meta_path()?] {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(e).with_context(|| format!("failed to remove {}", path.display()))
            }
        }
    }
    Ok(())
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
