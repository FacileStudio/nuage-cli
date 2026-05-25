mod api;
mod config;
mod daemon;
mod hash;
mod ignore;
mod sync;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::io::{self, Write};
use tracing::{error, info};

#[derive(Parser)]
#[command(name = "nuage", about = "File sync daemon for Nuage")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Start background daemon
    Start,
    /// Stop background daemon
    Stop,
    /// Restart background daemon
    Restart,
    /// Run one-time sync
    Sync,
    /// Start foreground watcher (for debugging)
    Watch,
    /// Show sync and daemon status
    Status,
    /// Show daemon logs
    Logs(LogsArgs),
    /// Interactive login setup
    Login,
    /// Upgrade nuage-cli
    Upgrade,
}

#[derive(clap::Args)]
struct LogsArgs {
    #[arg(short, long)]
    follow: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Start) => cmd_start(),
        Some(Command::Stop) => cmd_stop(),
        Some(Command::Restart) => cmd_restart(),
        Some(Command::Logs(args)) => cmd_logs(args.follow),
        other => {
            daemon::init_terminal_logging();

            let rt = tokio::runtime::Runtime::new()
                .context("failed to create async runtime")?;

            rt.block_on(async {
                match other {
                    None | Some(Command::Watch) => cmd_watch().await,
                    Some(Command::Sync) => cmd_sync().await,
                    Some(Command::Status) => cmd_status().await,
                    Some(Command::Login) => cmd_login().await,
                    Some(Command::Upgrade) => cmd_upgrade().await,
                    _ => unreachable!(),
                }
            })
        }
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

fn cmd_start() -> Result<()> {
    if let Some(pid) = daemon::is_running()? {
        println!("[nuage] already running (PID {})", pid);
        return Ok(());
    }

    config::Config::load().context("fix config before starting daemon")?;

    let log_dir = daemon::log_dir()?;
    std::fs::create_dir_all(&log_dir)?;

    let log_file = daemon::log_path()?;
    let stdout = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
        .with_context(|| format!("cannot open log file: {}", log_file.display()))?;
    let stderr = stdout
        .try_clone()
        .context("failed to clone log file handle")?;

    let pid_file = daemon::pid_path()?;

    println!("[nuage] starting daemon...");

    let daemonize = daemonize::Daemonize::new()
        .pid_file(&pid_file)
        .chown_pid_file(true)
        .stdout(stdout)
        .stderr(stderr);

    daemonize.start().context("failed to daemonize")?;

    daemon::init_daemon_logging();

    let rt = tokio::runtime::Runtime::new()
        .context("failed to create async runtime")?;

    rt.block_on(run_daemon())
}

fn cmd_stop() -> Result<()> {
    match daemon::is_running()? {
        Some(pid) => {
            let kill_result = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
            if kill_result != 0 {
                let _ = std::fs::remove_file(daemon::pid_path()?);
                println!("[nuage] process already gone, cleaned up PID file");
                return Ok(());
            }

            for _ in 0..50 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                let alive = unsafe { libc::kill(pid as i32, 0) == 0 };
                if !alive {
                    let _ = std::fs::remove_file(daemon::pid_path()?);
                    println!("[nuage] stopped (was PID {})", pid);
                    return Ok(());
                }
            }

            unsafe {
                libc::kill(pid as i32, libc::SIGKILL);
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
            let _ = std::fs::remove_file(daemon::pid_path()?);
            println!("[nuage] killed (PID {})", pid);
            Ok(())
        }
        None => {
            println!("[nuage] not running");
            Ok(())
        }
    }
}

fn cmd_restart() -> Result<()> {
    cmd_stop()?;
    cmd_start()
}

fn cmd_logs(follow: bool) -> Result<()> {
    let log_file = daemon::log_path()?;
    if !log_file.exists() {
        println!("[nuage] no logs yet");
        return Ok(());
    }

    let mut args = vec![];
    if follow {
        args.extend(["-f", "-n", "50"]);
    } else {
        args.extend(["-n", "50"]);
    }
    let path_str = log_file.to_string_lossy().to_string();
    args.push(&path_str);

    let status = std::process::Command::new("tail")
        .args(&args)
        .status()
        .context("failed to run tail")?;

    if !status.success() {
        bail!("tail exited with error");
    }
    Ok(())
}

async fn sync_loop(engine: &sync::SyncEngine) -> Result<()> {
    let sync_dir = engine.sync_dir().to_path_buf();
    let poll_interval = engine.config().poll_interval;

    let watcher = sync::watcher::FsWatcher::new(&sync_dir, engine.ignore_rules())?;

    let mut poll_timer =
        tokio::time::interval(tokio::time::Duration::from_secs(poll_interval));
    poll_timer.tick().await;

    let mut sigterm =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut sigint =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;

    loop {
        if let Some(paths) = watcher.try_recv() {
            if let Err(e) = engine.process_local_changes(paths).await {
                error!("local sync error: {}", e);
            }
        }

        tokio::select! {
            _ = sigterm.recv() => {
                info!("shutting down (SIGTERM)");
                break;
            }
            _ = sigint.recv() => {
                info!("shutting down (SIGINT)");
                break;
            }
            _ = poll_timer.tick() => {
                match engine.process_remote_changes().await {
                    Ok(report) => {
                        let total = report.downloaded + report.uploaded
                            + report.deleted_local + report.deleted_remote;
                        if total > 0 {
                            info!("sync ({} changes)", total);
                        }
                    }
                    Err(e) => error!("remote sync error: {}", e),
                }
            }
            _ = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {}
        }
    }

    Ok(())
}

async fn run_daemon() -> Result<()> {
    let engine = build_engine()?;

    info!("daemon started, PID {}", std::process::id());

    let report = engine.full_sync().await?;
    let file_count = engine.state().file_count().unwrap_or(0);
    info!("watching {} ({} files synced)", engine.sync_dir().display(), file_count);

    if report.conflicts > 0 {
        info!("{} conflicts resolved", report.conflicts);
    }

    sync_loop(&engine).await?;

    let _ = std::fs::remove_file(daemon::pid_path()?);
    info!("daemon stopped");
    Ok(())
}

async fn cmd_watch() -> Result<()> {
    let engine = build_engine()?;

    println!("[nuage] starting initial sync...");
    let report = engine.full_sync().await?;

    let file_count = engine.state().file_count().unwrap_or(0);
    println!(
        "[nuage] watching {} (synced {} files)",
        engine.sync_dir().display(),
        file_count
    );

    if report.conflicts > 0 {
        println!("[nuage] {} conflicts resolved", report.conflicts);
    }

    sync_loop(&engine).await?;

    println!("\n[nuage] stopped");
    Ok(())
}

async fn cmd_sync() -> Result<()> {
    let engine = build_engine()?;

    println!("[nuage] syncing...");
    let report = engine.full_sync().await?;

    let total = report.downloaded + report.uploaded + report.deleted_local + report.deleted_remote;
    println!("[nuage] sync complete ({} changes)", total);

    if report.conflicts > 0 {
        println!("[nuage] {} conflicts resolved (local copies renamed)", report.conflicts);
    }

    Ok(())
}

async fn cmd_status() -> Result<()> {
    match daemon::is_running()? {
        Some(pid) => println!("Daemon: running (PID {})", pid),
        None => println!("Daemon: stopped"),
    }

    let config = config::Config::load()?;
    let sync_dir = config.sync_dir_expanded()?;

    if !sync_dir.join(".nuage").join("state.db").exists() {
        println!("Server: {}", config.server_url);
        println!("Sync dir: {}", sync_dir.display());
        println!("Last sync: never");
        println!("Files: 0");
        println!("Folders: 0");
        return Ok(());
    }

    let state = sync::state::SyncState::new(&sync_dir)?;
    let cursor = state.get_cursor()?.unwrap_or_else(|| "never".to_string());
    let file_count = state.file_count()?;
    let folder_count = state.folder_count()?;

    println!("Server: {}", config.server_url);
    println!("Sync dir: {}", sync_dir.display());
    println!("Last sync: {}", cursor);
    println!("Files: {}", file_count);
    println!("Folders: {}", folder_count);

    Ok(())
}

async fn cmd_login() -> Result<()> {
    println!("nuage -- interactive setup\n");

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
    println!("connected successfully");

    config.save()?;
    println!("config saved to ~/.nuage.yml");

    let expanded = shellexpand::tilde(&sync_dir);
    let sync_path = std::path::PathBuf::from(expanded.as_ref());
    std::fs::create_dir_all(&sync_path)
        .with_context(|| format!("cannot create sync directory: {}", sync_path.display()))?;
    println!("sync directory ready: {}", sync_path.display());

    println!("\nRun `nuage start` to start syncing in the background.");
    println!("Run `nuage watch` for foreground mode.");
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
