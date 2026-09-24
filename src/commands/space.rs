use anyhow::{bail, Result};
use std::sync::OnceLock;

use crate::api::ApiClient;
use crate::config::{self, env_space};

/// The name for the account's own tree.
///
/// The server does not list it, because it is the absence of a space rather
/// than one of them. Without a name for it there is no way to ask for it by
/// hand, which left `--none` as the only way back and nothing in `spaces list`
/// saying so.
pub const PERSONAL: &str = "personal";

pub fn is_personal(raw: &str) -> bool {
    raw.eq_ignore_ascii_case(PERSONAL)
}

/// What a space reference names, before any lookup happens.
///
/// An id costs nothing to use and a name costs one request, so the two are kept
/// apart until a request is actually available.
pub enum SpaceRef {
    Personal,
    Id(i64),
    Name(String),
}

pub fn parse_ref(raw: &str) -> SpaceRef {
    if is_personal(raw) {
        SpaceRef::Personal
    } else if let Ok(id) = raw.parse::<i64>() {
        SpaceRef::Id(id)
    } else {
        SpaceRef::Name(raw.to_string())
    }
}

/// The `NUAGE_SPACE` override, resolved to an id once per run.
///
/// Resolving a name costs a request, and `load_api` is called from the
/// synchronous command bodies, so the lookup happens once in `run_async` and
/// every later caller reads the answer.
static OVERRIDE: OnceLock<Option<i64>> = OnceLock::new();

pub fn set_override(space: Option<i64>) {
    let _ = OVERRIDE.set(space);
}

/// The space this run's commands act on: `NUAGE_SPACE` when it names one, the
/// personal space otherwise.
pub fn override_space() -> Option<i64> {
    OVERRIDE.get().copied().flatten()
}

pub fn load_api() -> Result<ApiClient> {
    let config = config::Config::load()?;
    ApiClient::new(&config.server_url, &config.token, override_space())
}

pub async fn resolve_env_space() -> Result<Option<i64>> {
    let Some(raw) = env_space() else {
        return Ok(None);
    };

    match parse_ref(&raw) {
        SpaceRef::Personal => Ok(None),
        SpaceRef::Id(id) => Ok(Some(id)),
        SpaceRef::Name(name) => {
            let config = config::Config::load()?;
            let api = ApiClient::new(&config.server_url, &config.token, None)?;
            Ok(Some(resolve_space_name(&api, &name).await?))
        }
    }
}

/// Matches a space by name, case-insensitively.
pub async fn resolve_space_name(api: &ApiClient, name: &str) -> Result<i64> {
    let spaces = api.list_spaces().await?;
    let wanted = name.to_lowercase();

    match spaces.iter().find(|s| s.name.to_lowercase() == wanted) {
        Some(space) => Ok(space.id),
        None if spaces.is_empty() => {
            bail!("no space named `{name}` — this account belongs to none, so only `{PERSONAL}` is available")
        }
        None => {
            let mut known: Vec<&str> = vec![PERSONAL];
            known.extend(spaces.iter().map(|s| s.name.as_str()));
            bail!("no space named `{name}` — known: {}", known.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reference_is_read_before_any_lookup() {
        assert!(is_personal("PERSONAL"));
        assert!(matches!(parse_ref("personal"), SpaceRef::Personal));
        assert!(matches!(parse_ref("7"), SpaceRef::Id(7)));
        assert!(matches!(parse_ref("FacileShared"), SpaceRef::Name(_)));
    }
}
