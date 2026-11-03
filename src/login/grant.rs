use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::time::{Duration, Instant};

use super::poll::{next, Next};

const ISSUER: &str = "https://sso.facile.studio";
const CLIENT_ID: &str = "facile-cli";
const SCOPES: &str = "openid email profile";
const GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";
const DEFAULT_INTERVAL: u64 = 5;
const DEFAULT_EXPIRY: u64 = 600;

/// The two endpoints of Registre's RFC 8628 device grant, read from its root
/// discovery document. Per-application discovery paths answer with HTML.
pub(super) struct Provider {
    device_authorization: String,
    token: String,
}

/// The device authorization response, RFC 8628 §3.2. `link` carries the user
/// code inside it, so nobody has to type the code.
pub(super) struct Authorization {
    device_code: String,
    pub(super) user_code: String,
    pub(super) link: String,
    interval: Duration,
    expires: Duration,
}

impl Provider {
    /// Reads the endpoints from `https://sso.facile.studio/.well-known/openid-configuration`.
    pub(super) async fn discover() -> Result<Self> {
        let doc: Value = reqwest::Client::new()
            .get(format!("{ISSUER}/.well-known/openid-configuration"))
            .send()
            .await
            .context("cannot reach the identity provider")?
            .error_for_status()
            .context("the identity provider served no OpenID configuration")?
            .json()
            .await
            .context("the identity provider's OpenID configuration is not JSON")?;
        let provider = Self {
            device_authorization: text(&doc, "device_authorization_endpoint"),
            token: text(&doc, "token_endpoint"),
        };
        if provider.device_authorization.is_empty() || provider.token.is_empty() {
            bail!("{ISSUER} does not offer a device sign-in");
        }
        Ok(provider)
    }

    /// Starts a device sign-in as the public `facile-cli` client, which has no
    /// secret. The approval link must be https, because the CLI hands it to the
    /// operating system to open.
    pub(super) async fn authorize(&self) -> Result<Authorization> {
        let (status, doc) = post_form(
            &self.device_authorization,
            &[("client_id", CLIENT_ID), ("scope", SCOPES)],
        )
        .await?;
        if !(200..300).contains(&status) {
            bail!("the identity provider would not start a device sign-in ({status})");
        }
        let mut link = text(&doc, "verification_uri_complete");
        if link.is_empty() {
            link = text(&doc, "verification_uri");
        }
        let auth = Authorization {
            device_code: text(&doc, "device_code"),
            user_code: text(&doc, "user_code"),
            link,
            interval: seconds(&doc, "interval", DEFAULT_INTERVAL),
            expires: seconds(&doc, "expires_in", DEFAULT_EXPIRY),
        };
        if auth.device_code.is_empty() || !auth.link.starts_with("https://") {
            bail!("the identity provider's device authorization is not usable");
        }
        Ok(auth)
    }

    /// Polls the token endpoint until the user approves, refuses, or the code
    /// expires. A transport error is retried: the deadline already bounds the
    /// loop.
    pub(super) async fn await_token(&self, auth: &Authorization) -> Result<String> {
        let deadline = Instant::now() + auth.expires;
        let mut interval = auth.interval;
        let form = [
            ("grant_type", GRANT_TYPE),
            ("device_code", auth.device_code.as_str()),
            ("client_id", CLIENT_ID),
        ];
        while Instant::now() < deadline {
            tokio::time::sleep(interval).await;
            let Ok((status, doc)) = post_form(&self.token, &form).await else {
                continue;
            };
            match next(interval, status, &doc)? {
                Next::Done(token) => return Ok(token),
                Next::Wait(wait) => interval = wait,
            }
        }
        bail!("the code expired before it was approved, run `nuage login` again")
    }
}

async fn post_form(url: &str, form: &[(&str, &str)]) -> Result<(u16, Value)> {
    let response = reqwest::Client::new()
        .post(url)
        .header("Accept", "application/json")
        .form(form)
        .send()
        .await
        .context("cannot reach the identity provider")?;
    let status = response.status().as_u16();
    Ok((status, response.json().await.unwrap_or(Value::Null)))
}

pub(super) fn text(doc: &Value, key: &str) -> String {
    doc.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn seconds(doc: &Value, key: &str, fallback: u64) -> Duration {
    let value = doc
        .get(key)
        .and_then(Value::as_u64)
        .filter(|v| *v > 0)
        .unwrap_or(fallback);
    Duration::from_secs(value)
}
