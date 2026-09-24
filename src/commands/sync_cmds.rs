use anyhow::Result;
use clap::Args;
use std::path::Path;

use crate::commands::daemon_run::sync_loop;
use crate::commands::space::build_engine;
use crate::sync;
use crate::sync::state::SyncState;
use crate::ui;

#[derive(Args)]
pub struct SyncArgs {
    #[arg(long, help = "Show what would change without applying anything")]
    pub dry_run: bool,
    #[arg(
        long,
        help = "Allow propagating an unusually large batch of local deletions"
    )]
    pub allow_bulk_delete: bool,
    #[arg(long, help = "Clear quarantined files and retry them")]
    pub retry_failed: bool,
    #[arg(
        long,
        help = "Drop tracking records whose local file is gone, without deleting anything on the server, then re-enumerate"
    )]
    pub repair_state: bool,
}

fn warn_blocked_deletes(report: &sync::SyncReport) {
    if report.blocked_deletes > 0 {
        ui::warn(&format!(
            "{} deletion(s) held back by the safety guard — see `nuage sync --dry-run`",
            report.blocked_deletes
        ));
    }
}

pub fn report_warnings(report: &sync::SyncReport) {
    if report.conflicts > 0 {
        ui::warn(&format!(
            "{} conflict(s) — both versions kept, local copy renamed",
            report.conflicts
        ));
    }
    warn_blocked_deletes(report);
    if report.skipped > 0 {
        ui::warn(&format!(
            "{} quarantined file(s) skipped — retry with `nuage sync --retry-failed`",
            report.skipped
        ));
    }
    if report.errors > 0 {
        ui::warn(&format!("{} item(s) failed this pass", report.errors));
    }
}

pub async fn cmd_watch() -> Result<()> {
    let engine = build_engine()?;
    engine.preflight()?;

    ui::step("Starting initial sync");
    let report = engine.full_sync().await?;

    let file_count = engine.state().file_count().unwrap_or(0);
    println!(
        "[nuage] watching {} (synced {} files)",
        engine.sync_dir().display(),
        file_count
    );

    report_warnings(&report);

    sync_loop(&engine).await?;

    println!("\n[nuage] stopped");
    Ok(())
}

fn clear_quarantine(engine: &sync::SyncEngine) -> Result<()> {
    let cleared = engine.state().clear_all_quarantine()?;
    if cleared > 0 {
        ui::step(&format!("Cleared {} quarantined file(s)", cleared));
    }
    Ok(())
}

fn report_sync(report: &sync::SyncReport, dry_run: bool) {
    if !dry_run {
        ui::success(&format!(
            "Sync complete ({} changes)",
            report.total_changes()
        ));
        return;
    }

    if report.planned.is_empty() {
        ui::success("Already in sync — no changes planned");
        return;
    }

    for line in &report.planned {
        println!("  {}", line);
    }
    ui::success(&format!("{} change(s) planned", report.planned.len()));
}

pub async fn cmd_sync(args: &SyncArgs) -> Result<()> {
    let options = sync::SyncOptions {
        dry_run: args.dry_run,
        allow_bulk_delete: args.allow_bulk_delete,
    };
    let engine = build_engine()?.with_options(options);
    engine.preflight()?;

    if args.retry_failed {
        clear_quarantine(&engine)?;
    }

    if args.repair_state {
        repair_state(&engine, args.dry_run)?;
    }

    if args.dry_run {
        ui::step("Dry run — nothing will be modified");
    } else {
        ui::step("Syncing");
    }

    let report = engine.full_sync().await?;

    report_sync(&report, args.dry_run);
    report_warnings(&report);
    Ok(())
}

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
fn repair_state(engine: &sync::SyncEngine, dry_run: bool) -> Result<()> {
    let sync_dir = engine.sync_dir();
    let state = engine.state();

    let (stale_files, stale_folders) = stale_records(state, sync_dir)?;

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
        state.remove_file(path)?;
    }
    for path in &stale_folders {
        state.remove_folder(path)?;
    }
    state.clear_cursor()?;

    ui::success(&format!(
        "Dropped {} stale file record(s) and {} stale folder record(s) — the server was not modified",
        stale_files.len(),
        stale_folders.len()
    ));
    Ok(())
}
