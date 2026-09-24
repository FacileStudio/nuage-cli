use anyhow::Result;

use super::meta::clear_runtime_files;
use super::paths::pid_path;

/// Whether a daemon is running, and its pid when one is.
///
/// The pid file alone cannot answer this. It is written by `daemonize` and
/// removed when the daemon exits, so a daemon that died without cleanup leaves
/// it behind, and once the kernel recycles that pid the file points at an
/// unrelated process. Checking only that *a* process named `nuage` is alive
/// answers "running" in that case, which makes `start` refuse to start and
/// makes `stop` signal a process that is not the daemon.
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

    match is_daemon_process(pid) {
        Some(true) | None => Ok(Some(pid)),
        Some(false) => {
            let _ = clear_runtime_files();
            Ok(None)
        }
    }
}

/// The subcommands that fork a daemon, and so the argv a live daemon can show.
///
/// `daemonize` keeps the argv it was forked with, so a daemon started by
/// `nuage upgrade` still reads as `nuage upgrade` for the rest of its life.
/// Matching only `start` would report that daemon as stopped, and `start`
/// would then run a second daemon over the same directories and state database.
const DAEMON_SUBCOMMANDS: [&str; 3] = ["start", "restart", "upgrade"];

/// Whether the process at `pid` is a daemon this CLI started.
///
/// `None` means the answer could not be determined, which is treated as
/// running: guessing "not running" there would start a second daemon over the
/// same directories and the same state database.
fn is_daemon_process(pid: u32) -> Option<bool> {
    let output = std::process::Command::new("ps")
        .arg("-p")
        .arg(pid.to_string())
        .arg("-o")
        .arg("args=")
        .output()
        .ok()?;

    let args = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if args.is_empty() {
        if output.status.success() {
            return None;
        }
        return Some(false);
    }

    Some(is_daemon_args(&args))
}

fn is_daemon_args(args: &str) -> bool {
    let mut tokens = args.split_whitespace();
    let exe = tokens.next().unwrap_or("");
    let base = exe.rsplit('/').next().unwrap_or(exe);

    base == "nuage"
        && tokens
            .next()
            .is_some_and(|sub| DAEMON_SUBCOMMANDS.contains(&sub))
}

#[cfg(test)]
mod tests {
    use super::*;

    // A daemon keeps the argv it was forked with, so every subcommand that can
    // fork one has to read as a daemon. Seeing `upgrade` here and calling it
    // stopped is what lets `nuage start` run a second daemon over the first.
    #[test]
    fn every_argv_a_forked_daemon_can_show_reads_as_a_daemon() {
        for args in [
            "nuage start",
            "nuage restart",
            "nuage upgrade",
            "/home/yann/.local/bin/nuage start",
            "nuage start --json",
        ] {
            assert!(is_daemon_args(args), "{args}");
        }
    }

    // The pid file outlives a daemon that died without cleanup, and the kernel
    // recycles pids, so the file can point at something that is not ours.
    #[test]
    fn one_shot_commands_and_other_programs_do_not() {
        for args in [
            "nuage",
            "nuage sync",
            "nuage search import",
            "nuage spaces list",
            "nuagectl start",
            "/usr/bin/postgres -D /var/lib/postgresql",
        ] {
            assert!(!is_daemon_args(args), "{args}");
        }
    }
}
