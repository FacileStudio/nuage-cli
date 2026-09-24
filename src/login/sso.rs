use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::time::Duration;
use tokio::net::TcpListener;

use crate::ui;

use super::loopback::{nonce, wait_for_code};

/// How long to wait for the browser to come back before giving up. Long enough
/// to type a password and answer a second factor, short enough that a closed
/// tab does not leave a listener open forever.
const WAIT: Duration = Duration::from_secs(180);

#[derive(Deserialize)]
struct ExchangeResponse {
    token: String,
}

/// What the server says it will accept. Absent or unreadable, we assume the
/// oldest shape — a Nuage that predates porte answers 404 here and still takes
/// an API token.
#[derive(Deserialize, Default)]
pub(super) struct AuthConfig {
    #[serde(default)]
    pub(super) sso_only: bool,
    #[serde(default)]
    pub(super) oidc_enabled: bool,
}

/// Runs the loopback handoff and returns the session token.
///
/// Port zero asks the kernel for a free one, so two shells can log in at the
/// same time without agreeing on anything.
///
/// The nonce is what makes the listener able to tell its own callback from one
/// somebody else sent. Without it any local process that guesses the port can
/// hand us a code of its choosing and we would exchange it.
pub(super) async fn sso(api: &str) -> Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("cannot open a loopback port to receive the login")?;
    let port = listener.local_addr()?.port();

    let state = nonce();

    let url = format!("{api}/auth/oidc?flow=cli&port={port}&cli_state={state}");
    ui::step("Opening the browser to sign in");
    ui::hint(&url);
    if open_browser(&url).is_err() {
        bail!("could not open a browser — paste that URL into one, or run `nuage login --token`");
    }

    let code = match tokio::time::timeout(WAIT, wait_for_code(listener, &state)).await {
        Ok(result) => result?,
        Err(_) => bail!("timed out waiting for the browser — run `nuage login` again"),
    };

    exchange(api, &code).await
}

/// Asks the server which flows it accepts, rather than asking the human what
/// their instance is configured for.
///
/// A server too old to answer is not a reason to refuse: it predates porte, so
/// it takes an API token and nothing else.
pub(super) async fn discover(api: &str) -> AuthConfig {
    let url = format!("{api}/auth/config");
    let Ok(response) = reqwest::Client::new().get(&url).send().await else {
        return AuthConfig::default();
    };
    if !response.status().is_success() {
        return AuthConfig::default();
    }
    response.json().await.unwrap_or_default()
}

async fn exchange(api: &str, code: &str) -> Result<String> {
    let url = format!("{api}/auth/oidc/exchange");
    let response = reqwest::Client::new()
        .post(&url)
        .json(&serde_json::json!({ "code": code }))
        .send()
        .await
        .context("cannot reach the server to exchange the login code")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        bail!("the server refused the login code ({status}): {body}");
    }
    let exchanged: ExchangeResponse = response
        .json()
        .await
        .context("the server's answer to the code exchange was not what was expected")?;
    if exchanged.token.is_empty() {
        bail!("the server returned an empty token — run `nuage login` again");
    }
    Ok(exchanged.token)
}

fn open_browser(url: &str) -> Result<()> {
    let command = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    let status = std::process::Command::new(command)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if !status.success() {
        bail!("browser command failed");
    }
    Ok(())
}
