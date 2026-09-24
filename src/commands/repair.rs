use anyhow::Result;
use std::path::Path;

use crate::sync::state::SyncState;
use crate::sync::SyncEngine;
use crate::ui;

fn stale_records(state: &SyncState, sync_dir: &Path) -> Result<(Vec<String>, Vec<String>)> {
    let stale_files = state
        .all_files()?
        .into_iter()
        .map(|r| r.local_path)
        .filter(|p| !sync_dir.join(p).exists())
        .collect();

    let stale_folders = state
        .all_folders()?
        .into_iter()
        .map(|r| r.local_path)
        .filter(|p| !sync_dir.join(p).is_dir())
        .collect();

    Ok((stale_files, stale_folders))
}

/// Drops tracking records that point at local files which no longer exist, leaving the
/// server untouched, then forgets the cursor so the next pass rebuilds tracking from the
/// server's own view. This recovers from state that drifted out of agreement with the
/// filesystem — for example records written at the sync root because a file's parent
/// folder could not be resolved at the time.
pub fn repair_state(engine: &SyncEngine, dry_run: bool) -> Result<()> {
    let (stale_files, stale_folders) = stale_records(engine.state(), &engine.target().dir)?;

    if stale_files.is_empty() && stale_folders.is_empty() {
        ui::step("State is consistent with the filesystem — nothing to repair");
        return Ok(());
    }

    if dry_run {
        ui::warn(&format!(
            "would drop {} stale file record(s) and {} stale folder record(s); the server would not be touched",
            stale_files.len(),
            stale_folders.len()
        ));
        return Ok(());
    }

    for path in &stale_files {
        engine.state().remove_file(path)?;
    }
    for path in &stale_folders {
        engine.state().remove_folder(path)?;
    }
    engine.state().clear_cursor()?;

    ui::success(&format!(
        "Dropped {} stale file record(s) and {} stale folder record(s) — the server was not modified",
        stale_files.len(),
        stale_folders.len()
    ));
    Ok(())
}
