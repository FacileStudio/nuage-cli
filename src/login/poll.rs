use anyhow::{bail, Result};
use serde_json::Value;
use std::time::Duration;

use super::grant::text;

/// RFC 8628 §3.5: every `slow_down` adds five seconds to the interval, for
/// this request and every one after it.
const SLOW_DOWN: Duration = Duration::from_secs(5);

/// What one poll of the token endpoint decides.
#[derive(Debug, PartialEq)]
pub(super) enum Next {
    Wait(Duration),
    Done(String),
}

/// Turns one token endpoint answer into the next step. The RFC 8628 errors
/// are not interchangeable: telling somebody their code expired when they
/// refused it sends them to retry what they meant to stop.
pub(super) fn next(interval: Duration, status: u16, doc: &Value) -> Result<Next> {
    if (200..300).contains(&status) {
        let token = text(doc, "access_token");
        if token.is_empty() {
            bail!("the identity provider approved this machine but returned no access token");
        }
        return Ok(Next::Done(token));
    }
    match text(doc, "error").as_str() {
        "authorization_pending" => Ok(Next::Wait(interval)),
        "slow_down" => Ok(Next::Wait(interval + SLOW_DOWN)),
        "access_denied" => bail!(
            "the sign-in was refused in the browser, run `nuage login` again if that was a mistake"
        ),
        "expired_token" => {
            bail!("the code expired before it was approved, run `nuage login` again")
        }
        other => bail!("the identity provider refused the device sign-in ({status} {other})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pending_then_slow_down_then_success() {
        let start = Duration::from_secs(5);
        let pending = next(start, 400, &json!({"error": "authorization_pending"})).unwrap();
        assert_eq!(pending, Next::Wait(start));
        let slower = next(start, 400, &json!({"error": "slow_down"})).unwrap();
        assert_eq!(slower, Next::Wait(Duration::from_secs(10)));
        let done = next(
            Duration::from_secs(10),
            200,
            &json!({"access_token": "abc"}),
        )
        .unwrap();
        assert_eq!(done, Next::Done("abc".into()));
    }

    #[test]
    fn expired_and_denied_stop_with_their_own_reason() {
        let start = Duration::from_secs(5);
        let expired = next(start, 400, &json!({"error": "expired_token"})).unwrap_err();
        assert!(expired.to_string().contains("expired"));
        let denied = next(start, 400, &json!({"error": "access_denied"})).unwrap_err();
        assert!(denied.to_string().contains("refused"));
        assert!(next(start, 200, &json!({})).is_err());
    }
}
