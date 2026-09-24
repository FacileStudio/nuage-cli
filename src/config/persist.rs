use anyhow::{bail, Context, Result};
use std::path::PathBuf;

use super::model::Config;

impl Config {
    pub fn path() -> Result<PathBuf> {
        let home = dirs::home_dir().context("cannot determine home directory")?;
        Ok(home.join(".nuage.yml"))
    }

    /// Resolves each space to an existing, canonical directory.
    ///
    /// The daemon runs one engine per entry, so two entries pointing at one
    /// directory, or at directories nested inside each other, would sync the
    /// same files twice and share one state database. Both are refused here
    /// rather than allowed to corrupt each other later.
    pub fn spaces_expanded(&self) -> Result<Vec<(String, PathBuf)>> {
        let mut expanded: Vec<(String, PathBuf)> = Vec::with_capacity(self.spaces.len());
        for (name, path) in &self.spaces {
            let target = PathBuf::from(shellexpand::tilde(path).as_ref());
            std::fs::create_dir_all(&target)
                .with_context(|| format!("cannot create {}", target.display()))?;
            let canonical = target
                .canonicalize()
                .with_context(|| format!("cannot resolve {}", target.display()))?;
            expanded.push((name.clone(), canonical));
        }
        for (index, (name, dir)) in expanded.iter().enumerate() {
            for (other_name, other_dir) in expanded.iter().skip(index + 1) {
                if dir.starts_with(other_dir) || other_dir.starts_with(dir) {
                    bail!(
                        "spaces `{name}` and `{other_name}` map to overlapping directories \
                         ({} and {}) — give each space its own directory in ~/.nuage.yml",
                        dir.display(),
                        other_dir.display()
                    );
                }
            }
        }
        Ok(expanded)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        let contents = serde_yaml::to_string(self).context("failed to serialize config")?;
        write_private(&path, contents.as_bytes())
            .with_context(|| format!("failed to write {}", path.display()))?;
        Ok(())
    }

    /// Tightens `~/.nuage.yml` to mode 0600 when it grants group or other permissions.
    ///
    /// Returns `Ok(true)` only when the permissions were actually changed.
    #[cfg(unix)]
    pub fn ensure_secure_permissions() -> Result<bool> {
        use std::os::unix::fs::PermissionsExt;

        let path = Self::path()?;
        let metadata = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e).with_context(|| format!("cannot stat {}", path.display())),
        };

        if metadata.permissions().mode() & 0o077 == 0 {
            return Ok(false);
        }

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to secure {}", path.display()))?;
        Ok(true)
    }

    /// Non-unix platforms have no mode bits to tighten, so this is always `Ok(false)`.
    #[cfg(not(unix))]
    pub fn ensure_secure_permissions() -> Result<bool> {
        Ok(false)
    }
}

#[cfg(unix)]
fn write_private(path: &std::path::Path, contents: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    file.write_all(contents)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn write_private(path: &std::path::Path, contents: &[u8]) -> Result<()> {
    std::fs::write(path, contents)?;
    Ok(())
}
