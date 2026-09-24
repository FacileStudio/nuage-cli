use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

use crate::commands::daemon_cmds::{cmd_start, cmd_stop};
use crate::daemon;
use crate::ui;

const REPO: &str = "https://github.com/FacileStudio/nuage-cli.git";

/// The directory the upgrade installs into, when it can be determined.
///
/// `cargo install` defaults to `~/.cargo/bin`, but this CLI is usually installed
/// elsewhere: `install.sh` writes `~/.local/bin`, and the daemon records the path
/// it was started from. Installing into the default replaced a binary nothing ran,
/// so the daemon kept serving the old build while the command reported success.
/// Preferring the daemon's path is what keeps the two in step.
fn install_bin_dir(daemon_exe: Option<&str>) -> Option<PathBuf> {
    let exe = daemon_exe
        .filter(|e| !e.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::current_exe().ok())?;

    let dir = exe.parent()?;
    if dir.file_name()? != "bin" {
        return None;
    }
    Some(dir.to_path_buf())
}

/// `cargo install` arguments, pinning `--root` to the parent of the binary
/// directory when one was resolved. A build tree keeps cargo's own default rather
/// than having `target/debug/../..` treated as an install root.
fn install_args(bin_dir: Option<&Path>) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = vec![
        "install".into(),
        "--git".into(),
        REPO.into(),
        "--force".into(),
    ];

    if let Some(root) = bin_dir.and_then(Path::parent) {
        args.push("--root".into());
        args.push(root.as_os_str().to_owned());
    }

    args
}

/// Brings the daemon back on the binary just installed rather than on the one this
/// process runs from, which may be a copy nothing else uses.
fn restart_from(bin_dir: Option<PathBuf>) -> Result<()> {
    let upgraded = bin_dir.map(|dir| dir.join("nuage")).filter(|p| p.is_file());

    let Some(path) = upgraded else {
        return cmd_start();
    };

    ui::step("Restarting daemon");
    let status = std::process::Command::new(&path)
        .arg("start")
        .status()
        .with_context(|| format!("failed to run {}", path.display()))?;

    if !status.success() {
        bail!("{} could not start the daemon", path.display());
    }
    Ok(())
}

/// Upgrades in place. The daemon is stopped first: replacing the executable of a
/// running process leaves it holding a half-written image, and a daemon left
/// running through an upgrade would keep serving the old code anyway.
pub async fn cmd_upgrade() -> Result<()> {
    let daemon_exe = daemon::read_meta()?.map(|meta| meta.exe);
    let was_running = daemon::is_running()?.is_some();
    if was_running {
        ui::step("Stopping daemon for upgrade");
        cmd_stop()?;
    }

    let bin_dir = install_bin_dir(daemon_exe.as_deref());

    ui::step("Upgrading nuage");
    if let Some(dir) = &bin_dir {
        ui::warn(&format!("Installing into {}", dir.display()));
    }

    let status = std::process::Command::new("cargo")
        .args(install_args(bin_dir.as_deref()))
        .status()
        .context("failed to run cargo install")?;

    if !status.success() {
        if was_running {
            ui::warn("Upgrade failed — restarting the previous daemon");
            let _ = cmd_start();
        }
        bail!("upgrade failed");
    }

    if was_running {
        restart_from(bin_dir)?;
    }

    ui::success("Upgraded to the latest version");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn the_upgrade_targets_the_directory_the_running_binary_lives_in() {
        assert_eq!(
            install_bin_dir(Some("/home/yann/.local/bin/nuage")),
            Some(PathBuf::from("/home/yann/.local/bin"))
        );
        assert_eq!(
            install_bin_dir(Some("/home/yann/.cargo/bin/nuage")),
            Some(PathBuf::from("/home/yann/.cargo/bin"))
        );
    }

    // A build tree has no `bin` directory, so cargo's own default is left alone
    // rather than having the repository root treated as an install root.
    #[test]
    fn a_build_tree_is_left_to_cargos_default() {
        assert_eq!(install_bin_dir(Some("target/debug/nuage")), None);
        assert_eq!(install_bin_dir(Some("nuage")), None);
    }

    #[test]
    fn the_install_root_is_the_parent_of_the_binary_directory() {
        let args = install_args(Some(Path::new("/home/yann/.local/bin")));
        let at = args
            .iter()
            .position(|arg| arg == "--root")
            .expect("--root is passed");
        assert_eq!(args[at + 1], OsString::from("/home/yann/.local"));
    }

    #[test]
    fn an_unresolved_directory_leaves_cargo_to_its_own_default() {
        let args = install_args(None);
        assert!(!args.iter().any(|arg| arg == "--root"));
    }
}
