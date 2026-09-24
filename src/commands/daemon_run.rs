use anyhow::Result;
use std::time::Duration;
use tokio::signal::unix::Signal;
use tracing::{error, info, warn};

use crate::commands::space::build_engine;
use crate::config;
use crate::daemon;
use crate::sync;

fn warn_blocked_deletes(report: &sync::SyncReport) {
    if report.blocked_deletes > 0 {
        warn!(
            "{} deletion(s) held back by the safety guard — run `nuage sync --dry-run` to inspect",
            report.blocked_deletes
        );
    }
}

fn log_local_result(result: Result<()>) {
    if let Err(e) = result {
        error!("local sync error: {}", e);
    }
}

async fn poll_once(engine: &sync::SyncEngine) {
    match engine.full_sync().await {
        Ok(report) => {
            if report.total_changes() > 0 {
                info!("sync ({} changes)", report.total_changes());
            }
            warn_blocked_deletes(&report);
        }
        Err(e) => error!("remote sync error: {}", e),
    }
}

async fn wait_local(
    engine: &sync::SyncEngine,
    watcher: &sync::watcher::FsWatcher,
    sigterm: &mut Signal,
    sigint: &mut Signal,
) -> bool {
    let Some(paths) = watcher.try_recv() else {
        return false;
    };

    tokio::select! {
        biased;
        _ = sigterm.recv() => {
            info!("shutting down (SIGTERM)");
            true
        }
        _ = sigint.recv() => {
            info!("shutting down (SIGINT)");
            true
        }
        result = engine.process_local_changes(paths) => {
            log_local_result(result);
            false
        }
    }
}

pub async fn sync_loop(engine: &sync::SyncEngine) -> Result<()> {
    let sync_dir = engine.sync_dir().to_path_buf();
    let poll_interval = engine.config().poll_interval;

    let watcher = sync::watcher::FsWatcher::new(&sync_dir, engine.ignore_rules())?;

    let mut poll_timer = tokio::time::interval(Duration::from_secs(poll_interval));
    poll_timer.tick().await;

    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;

    loop {
        if wait_local(engine, &watcher, &mut sigterm, &mut sigint).await {
            break;
        }

        tokio::select! {
            biased;
            _ = sigterm.recv() => {
                info!("shutting down (SIGTERM)");
                break;
            }
            _ = sigint.recv() => {
                info!("shutting down (SIGINT)");
                break;
            }
            _ = poll_timer.tick() => poll_once(engine).await,
            _ = tokio::time::sleep(Duration::from_millis(100)) => {}
        }
    }

    Ok(())
}

pub async fn run_daemon() -> Result<()> {
    let engine = build_engine()?;
    engine.preflight()?;

    let _ = daemon::write_meta();
    if config::Config::ensure_secure_permissions().unwrap_or(false) {
        warn!(
            "tightened permissions on ~/.nuage.yml — it held an API token readable by other users"
        );
    }

    info!("daemon started, PID {}", std::process::id());

    let report = engine.full_sync().await?;
    let file_count = engine.state().file_count().unwrap_or(0);
    info!(
        "watching {} ({} files synced)",
        engine.sync_dir().display(),
        file_count
    );

    if report.conflicts > 0 {
        info!("{} conflicts resolved", report.conflicts);
    }
    warn_blocked_deletes(&report);

    sync_loop(&engine).await?;

    let _ = daemon::clear_runtime_files();
    info!("daemon stopped");
    Ok(())
}
