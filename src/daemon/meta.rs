use anyhow::{Context, Result};

use super::paths::{meta_path, nuage_dir, pid_path};

/// Metadata about the daemon process recorded next to the pid file.
#[derive(Debug, Clone)]
pub struct DaemonMeta {
    pub started_at: String,
    pub exe: String,
}

/// Writes the daemon start timestamp and executable path to `~/.nuage/nuage.meta`.
pub fn write_meta() -> Result<()> {
    let dir = nuage_dir()?;
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;

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
