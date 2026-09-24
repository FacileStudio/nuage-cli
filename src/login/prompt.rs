use anyhow::{bail, Context, Result};
use std::io::{self, IsTerminal, Write};

use crate::config::{self, Config};
use crate::ui;

const DEFAULT_SYNC_DIR: &str = "~/Nuage";

pub(super) fn prompt_token() -> Result<String> {
    if !io::stdin().is_terminal() {
        bail!("no terminal to read an API token from — set NUAGE_TOKEN instead");
    }
    ui::step("Paste an API token from the Nuage dashboard, under Settings then API");
    print!("API token: ");
    io::stdout().flush()?;
    let token = rpassword::read_password().context("cannot read the API token")?;
    let token = token.trim().to_string();
    if token.is_empty() {
        bail!("the API token was empty — mint one in the dashboard under Settings then API");
    }
    Ok(token)
}

/// Asks where the account's own tree should live.
///
/// Only the personal space is mapped here: `spaces:` is a map, and the other
/// spaces are added to it by hand or by `nuage spaces create` once the login
/// has a token to reach the server with.
pub(super) fn ask_sync_dir() -> Result<String> {
    if !io::stdin().is_terminal() {
        return Ok(DEFAULT_SYNC_DIR.to_string());
    }
    print!("Personal space directory [{DEFAULT_SYNC_DIR}]: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(DEFAULT_SYNC_DIR.to_string());
    }
    Ok(trimmed.to_string())
}

/// api_base is the value the rest of the CLI expects in `server_url`: the API
/// root, including `/api`.
///
/// Every request is built by appending a path to it verbatim — `ApiClient` uses
/// a bare `format!` — so a `server_url` of `https://nuage.example.com` silently
/// produces `https://nuage.example.com/sync/state`, which the SPA answers with
/// its own index page. Accepting the bare host on the command line and
/// normalising here is what makes `--server https://nuage.example.com` do the
/// obvious thing.
pub(super) fn api_base(server: &str) -> String {
    let trimmed = server.trim_end_matches('/');
    if trimmed.ends_with("/api") {
        return trimmed.to_string();
    }
    format!("{trimmed}/api")
}

pub(super) fn resolve_server(server: Option<String>) -> Result<String> {
    if let Some(server) = server {
        return Ok(server);
    }
    if let Some(url) = config::env_server_url() {
        return Ok(url);
    }
    if let Ok(existing) = Config::load_or_default() {
        if !existing.server_url.is_empty() {
            return Ok(existing.server_url);
        }
    }
    if io::stdin().is_terminal() {
        print!("Server URL: ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let trimmed = input.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    bail!("no server known — run `nuage login --server https://nuage.example.com`")
}

#[cfg(test)]
mod tests {
    use super::*;

    // server_url is appended to verbatim by ApiClient, so it has to be the API
    // root. Getting this wrong sends every later request to the SPA, and the
    // login itself would still appear to succeed.
    #[test]
    fn api_base_normalises_what_a_human_would_type() {
        for input in [
            "https://nuage.facile.studio",
            "https://nuage.facile.studio/",
            "https://nuage.facile.studio/api",
            "https://nuage.facile.studio/api/",
        ] {
            assert_eq!(
                api_base(input),
                "https://nuage.facile.studio/api",
                "{input}"
            );
        }
    }
}
