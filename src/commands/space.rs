use anyhow::{bail, Context, Result};
use std::sync::OnceLock;

use crate::api::ApiClient;
use crate::config;
use crate::ignore;
use crate::sync;

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

/// What `--space` asked for. `Unset` and `Personal` are different answers:
/// the first defers to the config, the second overrides it.
#[derive(Clone, Copy)]
pub enum SpaceFlag {
    Unset,
    Personal,
    Space(i64),
}

/// The `--space` flag, resolved to an id once per run.
///
/// Resolving a name costs a request, and `load_api` is called from twenty
/// synchronous places, so the lookup happens once in `run` and every later
/// caller reads the answer.
pub static SPACE_OVERRIDE: OnceLock<SpaceFlag> = OnceLock::new();

/// The space every request is scoped to: the flag when given, the config
/// otherwise, and the personal space when neither names one.
pub fn selected_space(config: &config::Config) -> Option<i64> {
    match SPACE_OVERRIDE.get() {
        Some(SpaceFlag::Personal) => None,
        Some(SpaceFlag::Space(id)) => Some(*id),
        _ => config.space,
    }
}

pub fn load_api() -> Result<ApiClient> {
    let config = config::Config::load()?;
    let space = selected_space(&config);
    ApiClient::new(&config.server_url, &config.token, space)
}

/// Builds the sync engine, deliberately unscoped.
///
/// `sync/state` without a space returns every space's tree, which is the merged
/// view `~/Nuage` already holds. Narrowing it would strand the files of every
/// other space in a directory the engine no longer tracks, so per-space sync
/// needs its own sync directory and its own change.
pub fn build_engine() -> Result<sync::SyncEngine> {
    let config = config::Config::load()?;
    let sync_dir = config.sync_dir_expanded()?;

    std::fs::create_dir_all(&sync_dir)
        .with_context(|| format!("cannot create sync directory: {}", sync_dir.display()))?;

    let api_client = ApiClient::new(&config.server_url, &config.token, None)?;
    let state = sync::state::SyncState::new(&sync_dir)?;
    let ignore = ignore::IgnoreRules::new(config.ignore_patterns.clone());

    sync::SyncEngine::new(config, api_client, state, ignore)
}

/// Turns a `--space` value into an id, hitting the server only for a name.
///
/// An id is the common case and costs nothing; a name costs one request, which
/// is the price of not having to look the number up by hand.
pub async fn resolve_space_flag(flag: Option<&str>) -> Result<SpaceFlag> {
    let raw = match flag {
        Some(v) => v.trim(),
        None => return Ok(SpaceFlag::Unset),
    };

    if is_personal(raw) {
        return Ok(SpaceFlag::Personal);
    }

    if let Ok(id) = raw.parse::<i64>() {
        return Ok(SpaceFlag::Space(id));
    }

    let config = config::Config::load()?;
    let api = ApiClient::new(&config.server_url, &config.token, None)?;
    Ok(SpaceFlag::Space(resolve_space_name(&api, raw).await?))
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
