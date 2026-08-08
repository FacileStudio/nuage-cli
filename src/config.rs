use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn default_sync_dir() -> String {
    "~/Nuage".to_string()
}

fn default_poll_interval() -> u64 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server_url: String,
    pub token: String,
    #[serde(default = "default_sync_dir")]
    pub sync_dir: String,
    #[serde(default = "default_poll_interval")]
    pub poll_interval: u64,
    #[serde(default)]
    pub ignore_patterns: Vec<String>,
    #[serde(default)]
    pub selective_sync: Vec<String>,
}

impl Config {
    pub fn path() -> Result<PathBuf> {
        let home = dirs::home_dir().context("cannot determine home directory")?;
        Ok(home.join(".nuage.yml"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        let contents = std::fs::read_to_string(&path).with_context(|| {
            format!(
                "cannot read {}\n\
                 Create ~/.nuage.yml with your server_url and token.\n\
                 Run `nuage login` for interactive setup.",
                path.display()
            )
        })?;
        let config: Self = serde_yaml::from_str(&contents)
            .with_context(|| format!("invalid config at {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        if self.server_url.is_empty() {
            bail!("server_url cannot be empty in ~/.nuage.yml");
        }
        if self.token.is_empty() {
            bail!("token cannot be empty in ~/.nuage.yml");
        }
        if !is_http_url(&self.server_url) {
            bail!(
                "server_url must be an http:// or https:// url in ~/.nuage.yml (got `{}`)",
                self.server_url
            );
        }
        if self.poll_interval == 0 {
            bail!("poll_interval must be at least 1 second in ~/.nuage.yml");
        }
        Ok(())
    }

    pub fn sync_dir_expanded(&self) -> Result<PathBuf> {
        let expanded = shellexpand::tilde(&self.sync_dir);
        Ok(PathBuf::from(expanded.as_ref()))
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
            Err(e) => {
                return Err(e).with_context(|| format!("cannot stat {}", path.display()))
            }
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

fn is_http_url(url: &str) -> bool {
    let rest = match url.strip_prefix("https://") {
        Some(r) => r,
        None => match url.strip_prefix("http://") {
            Some(r) => r,
            None => return false,
        },
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    !host.is_empty()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Config {
        Config {
            server_url: "https://nuage.example.com".to_string(),
            token: "tok".to_string(),
            sync_dir: default_sync_dir(),
            poll_interval: default_poll_interval(),
            ignore_patterns: vec![],
            selective_sync: vec![],
        }
    }

    #[test]
    fn accepts_well_formed_config() {
        assert!(sample().validate().is_ok());
    }

    #[test]
    fn rejects_empty_server_url() {
        let mut config = sample();
        config.server_url = String::new();
        let err = config.validate().unwrap_err().to_string();
        assert!(err.contains("server_url"));
    }

    #[test]
    fn rejects_zero_poll_interval() {
        let mut config = sample();
        config.poll_interval = 0;
        let err = config.validate().unwrap_err().to_string();
        assert!(err.contains("poll_interval"));
    }

    #[test]
    fn rejects_non_http_server_url() {
        let mut config = sample();
        config.server_url = "ftp://nuage.example.com".to_string();
        assert!(config.validate().is_err());

        config.server_url = "https://".to_string();
        assert!(config.validate().is_err());
    }
}
