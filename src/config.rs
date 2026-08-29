use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn default_sync_dir() -> String {
    "~/Nuage".to_string()
}

fn default_poll_interval() -> u64 {
    30
}

/// The credential, when the environment supplies one.
///
/// CI cannot run an interactive login and must not commit a config file, so an
/// env var is the only credential channel it has.
pub fn env_token() -> Option<String> {
    non_empty("NUAGE_TOKEN")
}

/// The instance, when the environment supplies one.
pub fn env_server_url() -> Option<String> {
    non_empty("NUAGE_SERVER_URL")
}

/// The space, when the environment supplies one.
///
/// An id rather than a name, because resolving a name costs a round-trip and
/// the environment channel exists for CI, which has an id to hand. A name is
/// refused rather than ignored: `--space` takes one, so a reader who assumes
/// this does too would otherwise get the personal space and no hint why.
pub fn env_space() -> Result<Option<i64>> {
    match non_empty("NUAGE_SPACE") {
        Some(raw) => parse_space(&raw).map(Some),
        None => Ok(None),
    }
}

fn parse_space(raw: &str) -> Result<i64> {
    match raw.parse::<i64>() {
        Ok(id) => Ok(id),
        Err(_) => bail!(
            "NUAGE_SPACE must be a space id, not a name (got `{raw}`) — `nuage spaces list` prints the ids"
        ),
    }
}

fn non_empty(key: &str) -> Option<String> {
    let value = std::env::var(key).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub server_url: String,
    #[serde(default)]
    pub token: String,
    #[serde(default = "default_sync_dir")]
    pub sync_dir: String,
    #[serde(default = "default_poll_interval")]
    pub poll_interval: u64,
    #[serde(default)]
    pub ignore_patterns: Vec<String>,
    #[serde(default)]
    pub selective_sync: Vec<String>,
    /// The space every request is scoped to, or the personal one when absent.
    ///
    /// Written by `nuage spaces use`, and skipped on serialize so a config that
    /// never selected one keeps the shape it already had.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space: Option<i64>,
}

/// A config nobody has written yet: the same field values the serde defaults
/// would produce, so a file-less run and a minimal file behave identically.
impl Default for Config {
    fn default() -> Self {
        Self {
            server_url: String::new(),
            token: String::new(),
            sync_dir: default_sync_dir(),
            poll_interval: default_poll_interval(),
            ignore_patterns: Vec::new(),
            selective_sync: Vec::new(),
            space: None,
        }
    }
}

impl Config {
    pub fn path() -> Result<PathBuf> {
        let home = dirs::home_dir().context("cannot determine home directory")?;
        Ok(home.join(".nuage.yml"))
    }

    pub fn load() -> Result<Self> {
        let mut config = Self::load_or_default()?;
        config.apply_env()?;
        config.validate()?;
        Ok(config)
    }

    /// Reads the config without validating it, treating an absent file as a
    /// fresh one.
    ///
    /// `login` and `logout` need this: refusing to run because the very field
    /// they are about to write is missing would make the config unrepairable by
    /// the command that exists to repair it. It is also the read half of the
    /// read-modify-write that keeps `sync_dir`, `ignore_patterns` and
    /// `selective_sync` — which belong to the user, not to the login — intact.
    pub fn load_or_default() -> Result<Self> {
        let path = Self::path()?;
        match std::fs::read_to_string(&path) {
            Ok(contents) => serde_yaml::from_str(&contents)
                .with_context(|| format!("invalid config at {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("cannot read {}", path.display())),
        }
    }

    /// Precedence is flag > environment > config file > built-in default. The
    /// flags are handled by the commands that take them, so by the time this
    /// runs the environment is the highest authority left.
    fn apply_env(&mut self) -> Result<()> {
        if let Some(url) = env_server_url() {
            self.server_url = url;
        }
        if let Some(token) = env_token() {
            self.token = token;
        }
        if let Some(space) = env_space()? {
            self.space = Some(space);
        }
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        if self.server_url.is_empty() {
            bail!(
                "no server_url configured — run `nuage login --server https://nuage.example.com`"
            );
        }
        if self.token.is_empty() {
            bail!("not signed in — run `nuage login`, or set NUAGE_TOKEN");
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
            space: None,
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

    // A login writes two fields and must leave the other four exactly as it
    // found them, so the parse it round-trips through has to tolerate a file
    // that is missing the credential it is about to supply.
    #[test]
    fn a_partial_file_keeps_the_user_settings_it_does_have() {
        let parsed: Config = serde_yaml::from_str(
            "server_url: https://nuage.example.com/api\nsync_dir: ~/Cloud\nselective_sync:\n  - Docs\n",
        )
        .unwrap();
        assert_eq!(parsed.token, "");
        assert_eq!(parsed.sync_dir, "~/Cloud");
        assert_eq!(parsed.selective_sync, vec!["Docs".to_string()]);
        assert_eq!(parsed.poll_interval, default_poll_interval());
        assert_eq!(parsed.space, None);
    }

    // A config that never selected a space must not grow the key on the next
    // write, or every login would start rewriting files it did not change.
    #[test]
    fn an_unselected_space_is_absent_from_the_written_file() {
        let yaml = serde_yaml::to_string(&sample()).unwrap();
        assert!(!yaml.contains("space"));

        let mut selected = sample();
        selected.space = Some(7);
        assert!(serde_yaml::to_string(&selected).unwrap().contains("space: 7"));
    }

    // A name in NUAGE_SPACE used to parse to None and leave the caller in the
    // personal space with nothing said, while `--space` accepted the same name.
    // Parsing is tested apart from the variable because the environment is
    // process-global and these tests run in parallel.
    #[test]
    fn a_name_where_a_space_id_belongs_is_refused_not_ignored() {
        assert_eq!(parse_space("7").unwrap(), 7);

        let err = parse_space("FacileShared").unwrap_err().to_string();
        assert!(err.contains("must be a space id"), "{err}");
        assert!(err.contains("FacileShared"), "{err}");
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
