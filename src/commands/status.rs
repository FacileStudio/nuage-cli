use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::config;
use crate::daemon;
use crate::sync::state::SyncState;

fn print_daemon_status() -> Result<()> {
    match daemon::is_running()? {
        Some(pid) => {
            println!("Daemon: running (PID {})", pid);
            if let Ok(Some(meta)) = daemon::read_meta() {
                println!("Started: {}", meta.started_at);
                println!("Binary: {}", meta.exe);
            }
        }
        None => println!("Daemon: stopped"),
    }
    Ok(())
}

fn print_selective(config: &config::Config) {
    if !config.selective_sync.is_empty() {
        println!("Selective sync: {}", config.selective_sync.join(", "));
    }
}

fn print_quarantine(state: &SyncState) -> Result<()> {
    let quarantined = state.list_quarantined()?;
    if quarantined.is_empty() {
        return Ok(());
    }

    println!("  Quarantined ({}):", quarantined.len());
    for record in &quarantined {
        println!(
            "    file {} — {} failure(s): {}",
            record.facile_id, record.attempts, record.reason
        );
    }
    println!("  Retry with `nuage sync --retry-failed`.");
    Ok(())
}

/// Reports one space from its own state database.
///
/// Read-only on purpose: `status` expands nothing and creates nothing, because
/// a missing directory is a fact worth reporting, not one worth papering over.
fn print_target(name: &str, dir: &Path) -> Result<()> {
    println!("\n{name}");
    println!("  Directory: {}", dir.display());

    if !dir.join(".nuage").join("state.db").exists() {
        println!("  Last sync: never");
        return Ok(());
    }

    let state = SyncState::new(dir)?;
    let cursor = state.get_cursor()?.unwrap_or_else(|| "never".to_string());
    println!("  Last sync: {cursor}");
    println!("  Files: {}", state.file_count()?);
    println!("  Folders: {}", state.folder_count()?);
    print_quarantine(&state)?;
    Ok(())
}

pub async fn cmd_status() -> Result<()> {
    print_daemon_status()?;

    let config = config::Config::load()?;
    println!("Server: {}", config.server_url);
    print_selective(&config);

    if config.spaces.is_empty() {
        println!("\nSpaces: none mapped — add a `spaces:` block to ~/.nuage.yml");
        return Ok(());
    }

    for (name, raw) in &config.spaces {
        let dir = PathBuf::from(shellexpand::tilde(raw).as_ref());
        print_target(name, &dir)?;
    }

    Ok(())
}
