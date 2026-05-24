mod api;
mod config;
mod hash;
mod ignore;
mod sync;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{error, info};

#[derive(Parser)]
#[command(name = "nuage", about = "File sync daemon for Nuage")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run one-time sync
    Sync,
    /// Start watching for changes (daemon mode)
    Watch,
    /// Show sync status
    Status,
    /// Interactive login setup
    Login,
    /// Upgrade nuage-cli
    Upgrade,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .with_target(false)
        .with_timer(tracing_subscriber::fmt::time::time())
        .init();

    let cli = Cli::parse();

    match cli.command {
        None | Some(Command::Watch) => cmd_watch().await,
        Some(Command::Sync) => cmd_sync().await,
        Some(Command::Status) => cmd_status().await,
        Some(Command::Login) => cmd_login().await,
        Some(Command::Upgrade) => cmd_upgrade().await,
    }
}

fn build_engine() -> Result<sync::SyncEngine> {
    let config = config::Config::load()?;
    let sync_dir = config.sync_dir_expanded()?;

    std::fs::create_dir_all(&sync_dir)
        .with_context(|| format!("cannot create sync directory: {}", sync_dir.display()))?;

    let api_client = api::ApiClient::new(&config.server_url, &config.token);
    let state = sync::state::SyncState::new(&sync_dir)?;
    let ignore = ignore::IgnoreRules::new(config.ignore_patterns.clone());

    sync::SyncEngine::new(config, api_client, state, ignore)
}

async fn cmd_sync() -> Result<()> {
    let engine = build_engine()?;

    println!("[nuage] syncing...");
    let report = engine.full_sync().await?;

    let total = report.downloaded + report.uploaded + report.deleted_local + report.deleted_remote;
    println!("[nuage] ✓ sync complete ({} changes)", total);

    if report.conflicts > 0 {
        println!("[nuage] ⚠ {} conflicts resolved (local copies renamed)", report.conflicts);
    }

    Ok(())
}

async fn cmd_watch() -> Result<()> {
    let engine = build_engine()?;
    let sync_dir = engine.sync_dir().to_path_buf();
    let poll_interval = engine.config().poll_interval;

    println!("[nuage] starting initial sync...");
    let report = engine.full_sync().await?;

    let file_count = engine.state().file_count().unwrap_or(0);
    println!("[nuage] watching {} (synced {} files)", sync_dir.display(), file_count);

    if report.conflicts > 0 {
        println!("[nuage] ⚠ {} conflicts resolved", report.conflicts);
    }

    let watcher = sync::watcher::FsWatcher::new(&sync_dir, engine.ignore_rules())?;

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .context("failed to set Ctrl+C handler")?;

    let mut poll_interval_timer = tokio::time::interval(
        tokio::time::Duration::from_secs(poll_interval),
    );
    poll_interval_timer.tick().await;

    while running.load(Ordering::SeqCst) {
        if let Some(paths) = watcher.try_recv() {
            if let Err(e) = engine.process_local_changes(paths).await {
                error!("local sync error: {}", e);
            }
        }

        tokio::select! {
            _ = poll_interval_timer.tick() => {
                match engine.process_remote_changes().await {
                    Ok(report) => {
                        let total = report.downloaded + report.uploaded + report.deleted_local + report.deleted_remote;
                        if total > 0 {
                            info!("✓ sync complete ({} changes)", total);
                        }
                    }
                    Err(e) => {
                        error!("remote sync error: {}", e);
                    }
                }
            }
            _ = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {}
        }
    }

    println!("\n[nuage] stopped");
    Ok(())
}

async fn cmd_status() -> Result<()> {
    let config = config::Config::load()?;
    let sync_dir = config.sync_dir_expanded()?;

    if !sync_dir.join(".nuage").join("state.db").exists() {
        println!("Connected to: {}", config.server_url);
        println!("Sync directory: {}", sync_dir.display());
        println!("Last sync: never");
        println!("Files tracked: 0");
        println!("Folders tracked: 0");
        return Ok(());
    }

    let state = sync::state::SyncState::new(&sync_dir)?;
    let cursor = state.get_cursor()?.unwrap_or_else(|| "never".to_string());
    let file_count = state.file_count()?;
    let folder_count = state.folder_count()?;

    println!("Connected to: {}", config.server_url);
    println!("Sync directory: {}", sync_dir.display());
    println!("Last sync: {}", cursor);
    println!("Files tracked: {}", file_count);
    println!("Folders tracked: {}", folder_count);

    Ok(())
}

async fn cmd_login() -> Result<()> {
    println!("nuage — interactive setup\n");

    let server_url = prompt("Server URL")?;
    if server_url.is_empty() {
        bail!("server URL cannot be empty");
    }

    let token = prompt("API token")?;
    if token.is_empty() {
        bail!("token cannot be empty");
    }

    let default_dir = "~/Nuage".to_string();
    let sync_dir_input = prompt_with_default("Sync directory", &default_dir)?;
    let sync_dir = if sync_dir_input.is_empty() {
        default_dir
    } else {
        sync_dir_input
    };

    let config = config::Config {
        server_url: server_url.clone(),
        token: token.clone(),
        sync_dir: sync_dir.clone(),
        poll_interval: 30,
        ignore_patterns: vec![
            ".DS_Store".to_string(),
            "*.tmp".to_string(),
            ".nuage/".to_string(),
            "Thumbs.db".to_string(),
            ".git/".to_string(),
        ],
    };

    println!("\nTesting connection...");
    let client = api::ApiClient::new(&server_url, &token);
    client.test_connection().await?;
    println!("✓ connected successfully");

    config.save()?;
    println!("✓ config saved to ~/.nuage.yml");

    let expanded = shellexpand::tilde(&sync_dir);
    let sync_path = std::path::PathBuf::from(expanded.as_ref());
    std::fs::create_dir_all(&sync_path)
        .with_context(|| format!("cannot create sync directory: {}", sync_path.display()))?;
    println!("✓ sync directory ready: {}", sync_path.display());

    println!("\nRun `nuage` to start syncing.");
    Ok(())
}

async fn cmd_upgrade() -> Result<()> {
    println!("Upgrading nuage...");
    let status = std::process::Command::new("cargo")
        .args([
            "install",
            "--git",
            "https://github.com/FacileStudio/nuage-cli.git",
            "--force",
            "--quiet",
        ])
        .status()?;
    if !status.success() {
        bail!("upgrade failed");
    }
    println!("Upgraded to latest version.");
    Ok(())
}

fn prompt(label: &str) -> Result<String> {
    print!("{}: ", label);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

fn prompt_with_default(label: &str, default: &str) -> Result<String> {
    print!("{} [{}]: ", label, default);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();
    if trimmed.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(trimmed.to_string())
    }
}
