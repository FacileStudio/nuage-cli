use anyhow::{Context, Result};
use std::time::Duration;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::watch;
use tracing::{error, info, warn};

use crate::sync::{SyncEngine, SyncReport};

/// Runs every target at once, under one shutdown signal.
///
/// Each target gets its own thread and its own current-thread runtime rather
/// than a `tokio::spawn`. The engine holds a SQLite connection, whose statement
/// cache is a `RefCell`, so it is `Send` but not `Sync` — and a future that
/// holds `&SyncEngine` across an await is neither. A thread per target also
/// gives real parallelism to the file scanning and hashing, which are blocking
/// calls made from inside the async pass.
///
/// The signal is broadcast rather than handled per target: two handlers on one
/// process race, and the second SIGTERM would find the first already consumed.
pub async fn run(engines: Vec<SyncEngine>) -> Result<()> {
    let (signal_tx, signal_rx) = watch::channel(false);
    let mut threads = Vec::with_capacity(engines.len());

    for engine in engines {
        let name = engine.target().name.clone();
        let shutdown = signal_rx.clone();
        let handle = std::thread::Builder::new()
            .name(format!("nuage-{name}"))
            .spawn(move || run_target(engine, shutdown))
            .context("cannot start a sync thread")?;
        threads.push((name, handle));
    }

    wait_for_signal(signal_tx).await?;

    for (name, thread) in threads {
        match thread.join() {
            Ok(Ok(())) => info!("{name}: stopped"),
            Ok(Err(e)) => error!("{name}: {e:#}"),
            Err(_) => error!("{name}: sync thread panicked"),
        }
    }

    Ok(())
}

fn run_target(engine: SyncEngine, shutdown: watch::Receiver<bool>) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("cannot start a sync runtime")?;
    runtime.block_on(target_loop(engine, shutdown))
}

async fn target_loop(engine: SyncEngine, mut shutdown: watch::Receiver<bool>) -> Result<()> {
    engine.preflight()?;
    initial_sync(&engine).await;

    let watcher = crate::sync::watcher::FsWatcher::new(&engine.target().dir, engine.ignore_rules())?;
    let mut poll = tokio::time::interval(Duration::from_secs(engine.config().poll_interval));
    poll.tick().await;

    loop {
        tokio::select! {
            biased;
            _ = shutdown.changed() => break,
            _ = poll.tick() => poll_once(&engine).await,
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                if let Some(paths) = watcher.try_recv() {
                    run_local(&engine, paths).await;
                }
            }
        }
    }

    Ok(())
}

async fn run_local(engine: &SyncEngine, paths: Vec<std::path::PathBuf>) {
    if let Err(e) = engine.process_local_changes(paths).await {
        error!("{}: local sync error: {e:#}", engine.target().name);
    }
}

async fn initial_sync(engine: &SyncEngine) {
    match engine.verify_remote().await {
        Ok(report) if report.total_changes() > 0 => info!(
            "{}: verified {} change(s) the incremental feed had lost",
            engine.target().name,
            report.total_changes()
        ),
        Ok(_) => {}
        Err(e) => error!("{}: verification failed: {e:#}", engine.target().name),
    }

    match engine.full_sync().await {
        Ok(report) => {
            info!(
                "{}: watching {} ({} files)",
                engine.target().name,
                engine.target().dir.display(),
                engine.state().file_count().unwrap_or(0)
            );
            warn_blocked(engine, &report);
        }
        Err(e) => error!("{}: initial sync failed: {e:#}", engine.target().name),
    }
}

async fn poll_once(engine: &SyncEngine) {
    match engine.full_sync().await {
        Ok(report) => {
            if report.total_changes() > 0 {
                info!(
                    "{}: sync ({} changes)",
                    engine.target().name,
                    report.total_changes()
                );
            }
            warn_blocked(engine, &report);
        }
        Err(e) => error!("{}: remote sync error: {e:#}", engine.target().name),
    }
}

fn warn_blocked(engine: &SyncEngine, report: &SyncReport) {
    if report.blocked_deletes > 0 {
        warn!(
            "{}: {} deletion(s) held back by the safety guard — run `nuage sync --dry-run` to inspect",
            engine.target().name, report.blocked_deletes
        );
    }
}

async fn wait_for_signal(done: watch::Sender<bool>) -> Result<()> {
    let mut sigterm = signal(SignalKind::terminate()).context("cannot listen for SIGTERM")?;
    let mut sigint = signal(SignalKind::interrupt()).context("cannot listen for SIGINT")?;

    tokio::select! {
        _ = sigterm.recv() => info!("shutting down (SIGTERM)"),
        _ = sigint.recv() => info!("shutting down (SIGINT)"),
    }

    let _ = done.send(true);
    Ok(())
}
