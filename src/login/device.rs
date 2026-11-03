use anyhow::{bail, Context, Result};

use crate::ui;

use super::grant::Provider;
use super::sso::{open_browser, ExchangeResponse};

/// Signs in with the RFC 8628 device grant and returns a Nuage session token,
/// or `None` when the server has not shipped the device exchange yet.
///
/// Nothing listens on this machine and nothing is pasted: the user follows
/// the link on any device with a browser, so it works over SSH. The exchange
/// route is probed first, so a server without it falls back before the user
/// approves anything.
pub(super) async fn device_login(api: &str) -> Result<Option<String>> {
    let url = format!("{api}/auth/oidc/device/exchange");
    if !exchange_mounted(post_exchange(&url, "").await?.status().as_u16()) {
        return Ok(None);
    }
    let provider = Provider::discover().await?;
    let auth = provider.authorize().await?;
    ui::step(&format!("Approve this sign-in in a browser: {}", auth.link));
    ui::hint(&format!(
        "The page shows the code {}. Waiting for approval.",
        auth.user_code
    ));
    let _ = open_browser(&auth.link);
    let access_token = provider.await_token(&auth).await?;
    exchange(api, &url, &access_token).await.map(Some)
}

/// The device exchange answers 404 only on a server that has not mounted it.
/// Any other status, a 401 for the empty probe included, means it exists.
fn exchange_mounted(status: u16) -> bool {
    status != 404
}

async fn post_exchange(url: &str, access_token: &str) -> Result<reqwest::Response> {
    reqwest::Client::new()
        .post(url)
        .json(&serde_json::json!({ "access_token": access_token }))
        .send()
        .await
        .context("cannot reach the server to exchange the sign-in")
}

async fn exchange(api: &str, url: &str, access_token: &str) -> Result<String> {
    let response = post_exchange(url, access_token).await?;
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        let site = api.trim_end_matches("/api");
        bail!("the server does not know this account yet. Sign in once at {site} in a browser, then run `nuage login` again");
    }
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        bail!("the server refused the sign-in ({status}): {}", body.trim());
    }
    let exchanged: ExchangeResponse = response
        .json()
        .await
        .context("the server's answer to the sign-in exchange was not what was expected")?;
    if exchanged.token.is_empty() {
        bail!("the server returned an empty token, run `nuage login` again");
    }
    Ok(exchanged.token)
}

#[cfg(test)]
mod tests {
    use super::exchange_mounted;

    #[test]
    fn only_a_404_falls_back_to_the_listener() {
        assert!(!exchange_mounted(404));
        assert!(exchange_mounted(401));
        assert!(exchange_mounted(400));
    }
}
