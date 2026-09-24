use anyhow::{bail, Result};
use std::path::PathBuf;

use crate::api::{ApiClient, ApiSpace};
use crate::commands::space::{parse_ref, SpaceRef};
use crate::config::Config;
use crate::ignore::IgnoreRules;
use crate::sync::{state::SyncState, SyncEngine, SyncTarget};

/// Every target the config maps, plus the names the server did not answer to.
///
/// An unknown name is not fatal on its own: a space someone else renamed, or
/// one this account lost access to, should cost that one target and leave the
/// rest syncing. The caller decides whether to warn or to refuse.
pub struct Targets {
    pub known: Vec<SyncTarget>,
    pub unknown: Vec<String>,
}

impl Targets {
    /// Names the spaces the server did not answer to, so a target that was
    /// dropped is never dropped silently.
    pub fn warn_unknown(&self) {
        for name in &self.unknown {
            crate::ui::warn(&format!(
                "no space named `{name}` — its directory was left alone"
            ));
        }
    }
}

pub async fn resolve_targets(config: &Config) -> Result<Targets> {
    let entries = config.spaces_expanded()?;
    let spaces = lookup_spaces(config, &entries).await?;

    let mut known = Vec::new();
    let mut unknown = Vec::new();
    for (name, dir) in entries {
        match space_for(&name, &spaces) {
            Some(space) => known.push(SyncTarget { name, space, dir }),
            None => unknown.push(name),
        }
    }

    require_any(&known, &unknown)?;
    Ok(Targets { known, unknown })
}

/// Builds one space's engine, scoped to that space and rooted at its directory.
pub fn build_engine(config: &Config, target: SyncTarget) -> Result<SyncEngine> {
    let api = ApiClient::new(&config.server_url, &config.token, target.space)?;
    let state = SyncState::new(&target.dir)?;
    let ignore = IgnoreRules::new(config.ignore_patterns.clone());
    Ok(SyncEngine::new(config.clone(), api, state, ignore, target))
}

async fn lookup_spaces(config: &Config, entries: &[(String, PathBuf)]) -> Result<Vec<ApiSpace>> {
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    let api = ApiClient::new(&config.server_url, &config.token, None)?;
    api.list_spaces().await
}

/// The space a configured name refers to.
///
/// The outer `Option` says whether the name is known at all; the inner one is
/// the space id, where `None` means the personal tree. The two are separate
/// answers, which is why this is not one flattened `Option<i64>`.
fn space_for(name: &str, spaces: &[ApiSpace]) -> Option<Option<i64>> {
    match parse_ref(name) {
        SpaceRef::Personal => Some(None),
        SpaceRef::Id(id) => Some(Some(id)),
        SpaceRef::Name(wanted) => find_space(spaces, &wanted).map(Some),
    }
}

fn find_space(spaces: &[ApiSpace], wanted: &str) -> Option<i64> {
    spaces
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(wanted))
        .map(|s| s.id)
}

fn require_any(known: &[SyncTarget], unknown: &[String]) -> Result<()> {
    if !known.is_empty() {
        return Ok(());
    }

    if unknown.is_empty() {
        bail!("no spaces mapped — add a `spaces:` block to ~/.nuage.yml, for example `personal: ~/Nuage`");
    }

    bail!(
        "none of the mapped spaces exist on this account: {}",
        unknown.join(", ")
    )
}
