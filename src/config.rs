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
        Ok(())
    }

    pub fn sync_dir_expanded(&self) -> Result<PathBuf> {
        let expanded = shellexpand::tilde(&self.sync_dir);
        Ok(PathBuf::from(expanded.as_ref()))
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        let contents = serde_yaml::to_string(self).context("failed to serialize config")?;
        std::fs::write(&path, contents)
            .with_context(|| format!("failed to write {}", path.display()))?;
        Ok(())
    }
}
