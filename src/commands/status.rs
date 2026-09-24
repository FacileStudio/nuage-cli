use anyhow::Result;
use std::path::Path;

use crate::commands::space::selected_space;
use crate::config;
use crate::daemon;
use crate::sync::state::SyncState;

/// How `status` names the current space, without spending a request on it.
fn describe_space(config: &config::Config) -> String {
    match selected_space(config) {
        Some(id) => format!("{id}"),
        None => "personal".to_string(),
    }
}

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

fn print_summary(config: &config::Config, sync_dir: &Path, cursor: &str, files: i64, folders: i64) {
    println!("Server: {}", config.server_url);
    println!("Space: {}", describe_space(config));
    println!("Sync dir: {}", sync_dir.display());
    println!("Last sync: {}", cursor);
    println!("Files: {}", files);
    println!("Folders: {}", folders);
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

    println!("\nQuarantined ({}):", quarantined.len());
    for record in &quarantined {
        println!(
            "  file {} — {} failure(s): {}",
            record.facile_id, record.attempts, record.reason
        );
    }
    println!("\nRetry with `nuage sync --retry-failed`.");
    Ok(())
}

pub async fn cmd_status() -> Result<()> {
    print_daemon_status()?;

    let config = config::Config::load()?;
    let sync_dir = config.sync_dir_expanded()?;

    if !sync_dir.join(".nuage").join("state.db").exists() {
        print_summary(&config, &sync_dir, "never", 0, 0);
        return Ok(());
    }

    let state = SyncState::new(&sync_dir)?;
    let cursor = state.get_cursor()?.unwrap_or_else(|| "never".to_string());
    print_summary(
        &config,
        &sync_dir,
        &cursor,
        state.file_count()?,
        state.folder_count()?,
    );

    print_selective(&config);
    print_quarantine(&state)?;
    Ok(())
}
