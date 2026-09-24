use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::env::{env_server_url, env_token};

fn default_poll_interval() -> u64 {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub server_url: String,
    #[serde(default)]
    pub token: String,
    /// Every space this account syncs, keyed by space name.
    ///
    /// Each value is the directory that space keeps in step with the server.
    /// The daemon runs one engine per entry. A `BTreeMap` orders the keys on
    /// write and makes a duplicate name impossible. An empty map is skipped so a
    /// config nobody has mapped a space in keeps the shape it already had.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub spaces: BTreeMap<String, String>,
    #[serde(default = "default_poll_interval")]
    pub poll_interval: u64,
    #[serde(default, alias = "ignore_patterns")]
    pub ignore: Vec<String>,
}

/// A config nobody has written yet: the same field values the serde defaults
/// would produce, so a file-less run and a minimal file behave identically.
impl Default for Config {
    fn default() -> Self {
        Self {
            server_url: String::new(),
            token: String::new(),
            spaces: BTreeMap::new(),
            poll_interval: default_poll_interval(),
            ignore: Vec::new(),
        }
    }
}

impl Config {
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
    /// read-modify-write that keeps `ignore` — which belongs to the user, not
    /// to the login — intact.
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

#[cfg(test)]
mod tests;
