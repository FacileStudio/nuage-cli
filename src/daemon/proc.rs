use anyhow::Result;

use super::meta::clear_runtime_files;
use super::paths::pid_path;

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
