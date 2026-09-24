use anyhow::{bail, Result};
use std::io::{self, IsTerminal};

use crate::api::ApiClient;
use crate::commands::space::PERSONAL;
use crate::config::{self, Config};
use crate::ui;

use super::prompt::{api_base, ask_sync_dir, prompt_token, resolve_server};
use super::sso::{discover, sso, AuthConfig};

/// What a fresh install starts with. Existing configs keep whatever the user
/// put there — a login has no business editing an ignore list.
const DEFAULT_IGNORE: [&str; 5] = [".DS_Store", "*.tmp", ".nuage/", "Thumbs.db", ".git/"];

/// Signs in and writes the credential into `~/.nuage.yml`, preserving every
/// field the login does not own.
///
/// The browser flow is the default because the suite runs `SSO_ONLY=true`: the
/// CLI never sees the identity provider, never handles a password, and never
/// holds an authorization code that is worth anything on its own. It opens a
/// loopback port, sends the browser to the API with that port and a nonce
/// attached, and the API — after the provider has done its part — redirects
/// back with a one-time code valid for sixty seconds and usable once.
///
/// `force_token` and a machine with no browser both fall back to pasting an API
/// token, because a headless box still needs a way in.
pub async fn run(server: Option<String>, force_token: bool) -> Result<()> {
    let fresh = !Config::path()?.exists();
    let server = resolve_server(server)?;
    let api = api_base(&server);

    let auth = discover(&api).await;
    let token = acquire_token(&api, &auth, force_token).await?;

    let mut config = Config::load_or_default()?;
    config.server_url = api.clone();
    config.token = token;
    if fresh {
        config.spaces.insert(PERSONAL.to_string(), ask_sync_dir()?);
        config.ignore = DEFAULT_IGNORE.iter().map(|p| p.to_string()).collect();
    }

    finish_login(&config).await
}

/// Blanks the token and leaves everything else alone.
///
/// Running it while already logged out is not an error: the state that makes
/// someone log out is often the state where they are unsure they are logged in.
pub fn logout() -> Result<()> {
    let mut config = Config::load_or_default()?;
    if config.token.is_empty() {
        ui::success("Already signed out");
        return Ok(());
    }

    config.token.clear();
    config.save()?;
    ui::success(&format!(
        "Signed out, token cleared from {}",
        Config::path()?.display()
    ));
    if config::env_token().is_some() {
        ui::warn("NUAGE_TOKEN is still set in this environment and overrides the config file");
    }
    Ok(())
}

async fn acquire_token(api: &str, auth: &AuthConfig, force_token: bool) -> Result<String> {
    if force_token || !auth.oidc_enabled {
        if auth.sso_only && !force_token {
            bail!("this instance accepts single sign-on only but did not advertise it — run `nuage login --token` with an API token from the dashboard");
        }
        return prompt_token();
    }

    match sso(api).await {
        Ok(token) => Ok(token),
        Err(err) if !auth.sso_only && io::stdin().is_terminal() => {
            ui::warn(&format!("{err:#}"));
            ui::hint("Falling back to an API token.");
            prompt_token()
        }
        Err(err) => Err(err),
    }
}

async fn finish_login(config: &Config) -> Result<()> {
    ui::step("Testing the connection");
    let client = ApiClient::new(&config.server_url, &config.token, None)?;
    client.test_connection().await?;

    config.save()?;
    ui::success(&format!(
        "Signed in, saved to {}",
        Config::path()?.display()
    ));

    for (name, dir) in config.spaces_expanded()? {
        ui::success(&format!("{name}: directory ready at {}", dir.display()));
    }
    if config.spaces.is_empty() {
        ui::warn("no spaces mapped — add a `spaces:` block to ~/.nuage.yml to sync anything");
    }
    ui::hint("Run `nuage start` to sync in the background, or `nuage watch` in the foreground.");
    Ok(())
}
