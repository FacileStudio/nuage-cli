use anyhow::Result;

use crate::api::ApiClient;
use crate::commands::space::resolve_space_name;
use crate::config;
use crate::ui;

/// Drops a space's directory from the `spaces:` block, matched case-insensitively.
///
/// A rename leaves the old name behind as a dead mapping, and a removed space
/// leaves a name the server no longer answers to. Neither is repaired by
/// itself, because the directory on disk is the user's and deleting a space on
/// the server says nothing about it.
pub(super) fn forget_mapping(name: &str) -> Result<()> {
    let mut config = config::Config::load_or_default()?;
    let before = config.spaces.len();
    config
        .spaces
        .retain(|key, _| !key.eq_ignore_ascii_case(name));

    if config.spaces.len() == before {
        return Ok(());
    }

    config.save()?;
    ui::step(&format!("removed `{name}` from the `spaces:` block"));
    Ok(())
}

/// Moves a space's directory onto its new name.
///
/// The `spaces:` block is keyed by name, so a rename on the server would strand
/// the old key: the space would stop counting as mapped and its directory would
/// keep syncing under a name nothing answers to.
pub(super) fn rename_mapping(old: &str, new: &str) -> Result<()> {
    let mut config = config::Config::load_or_default()?;
    let Some(dir) = config
        .spaces
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(old))
        .map(|(_, dir)| dir.clone())
    else {
        return Ok(());
    };

    config.spaces.retain(|key, _| !key.eq_ignore_ascii_case(old));
    config.spaces.insert(new.to_string(), dir);

    config.save()?;
    ui::step(&format!("moved `{old}` to `{new}` in the `spaces:` block"));
    Ok(())
}

/// Resolves a name or an id to both, since removing a space has to forget the
/// configured name as well as the server's id.
pub(super) async fn resolve_space_ref(api: &ApiClient, raw: &str) -> Result<(i64, Option<String>)> {
    if let Ok(id) = raw.parse::<i64>() {
        let name = api
            .list_spaces()
            .await?
            .into_iter()
            .find(|s| s.id == id)
            .map(|s| s.name);
        return Ok((id, name));
    }

    Ok((resolve_space_name(api, raw).await?, Some(raw.to_string())))
}
