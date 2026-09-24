use anyhow::{bail, Result};

/// The credential, when the environment supplies one.
///
/// CI cannot run an interactive login and must not commit a config file, so an
/// env var is the only credential channel it has.
pub fn env_token() -> Option<String> {
    non_empty("NUAGE_TOKEN")
}

/// The instance, when the environment supplies one.
pub fn env_server_url() -> Option<String> {
    non_empty("NUAGE_SERVER_URL")
}

/// The space, when the environment supplies one.
///
/// An id rather than a name, because resolving a name costs a round-trip and
/// the environment channel exists for CI, which has an id to hand. A name is
/// refused rather than ignored: `--space` takes one, so a reader who assumes
/// this does too would otherwise get the personal space and no hint why.
pub fn env_space() -> Result<Option<i64>> {
    match non_empty("NUAGE_SPACE") {
        Some(raw) => parse_space(&raw).map(Some),
        None => Ok(None),
    }
}

fn parse_space(raw: &str) -> Result<i64> {
    match raw.parse::<i64>() {
        Ok(id) => Ok(id),
        Err(_) => bail!(
            "NUAGE_SPACE must be a space id, not a name (got `{raw}`) — `nuage spaces list` prints the ids"
        ),
    }
}

fn non_empty(key: &str) -> Option<String> {
    let value = std::env::var(key).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // A name in NUAGE_SPACE used to parse to None and leave the caller in the
    // personal space with nothing said, while `--space` accepted the same name.
    // Parsing is tested apart from the variable because the environment is
    // process-global and these tests run in parallel.
    #[test]
    fn a_name_where_a_space_id_belongs_is_refused_not_ignored() {
        assert_eq!(parse_space("7").unwrap(), 7);

        let err = parse_space("FacileShared").unwrap_err().to_string();
        assert!(err.contains("must be a space id"), "{err}");
        assert!(err.contains("FacileShared"), "{err}");
    }
}
