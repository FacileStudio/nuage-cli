use anyhow::{bail, Context, Result};
use clap::Args;

use crate::commands::daemon_run::run_daemon;
use crate::config;
use crate::daemon;
use crate::ui;

#[derive(Args)]
pub struct LogsArgs {
    #[arg(short, long)]
    pub follow: bool,
}

pub fn cmd_start() -> Result<()> {
    if let Some(pid) = daemon::is_running()? {
        ui::warn(&format!("Already running (PID {})", pid));
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

    ui::step("Starting daemon");

    let daemonize = daemonize::Daemonize::new()
        .pid_file(&pid_file)
        .chown_pid_file(true)
        .stdout(stdout)
        .stderr(stderr);

    daemonize.start().context("failed to daemonize")?;

    daemon::init_daemon_logging();

    let rt = tokio::runtime::Runtime::new().context("failed to create async runtime")?;

    rt.block_on(run_daemon())
}

fn wait_for_exit(pid: u32) -> Result<()> {
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if unsafe { libc::kill(pid as i32, 0) } != 0 {
            let _ = daemon::clear_runtime_files();
            ui::success(&format!("Stopped (was PID {})", pid));
            return Ok(());
        }
    }

    unsafe {
        libc::kill(pid as i32, libc::SIGKILL);
    }
    std::thread::sleep(std::time::Duration::from_millis(200));
    let _ = daemon::clear_runtime_files();
    ui::success(&format!("Killed (PID {})", pid));
    Ok(())
}

pub fn cmd_stop() -> Result<()> {
    let Some(pid) = daemon::is_running()? else {
        ui::warn("Not running");
        return Ok(());
    };

    if unsafe { libc::kill(pid as i32, libc::SIGTERM) } != 0 {
        let _ = std::fs::remove_file(daemon::pid_path()?);
        ui::warn("Process already gone, cleaned up PID file");
        return Ok(());
    }

    wait_for_exit(pid)
}

pub fn cmd_restart() -> Result<()> {
    cmd_stop()?;
    cmd_start()
}

pub fn cmd_logs(follow: bool) -> Result<()> {
    let log_file = daemon::log_path()?;
    if !log_file.exists() {
        ui::step("No logs yet");
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
