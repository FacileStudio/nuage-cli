/// Checked in order, first non-empty wins.
const TOKEN_VARS: [&str; 3] = ["NUAGE_TOKEN", "NUAGE_API_KEY", "NUAGE_KEY"];

/// The credential, when the environment supplies one.
///
/// CI cannot run an interactive login and must not commit a config file, so an
/// env var is the only credential channel it has. `NUAGE_TOKEN` came first and
/// stays the documented name; the other two are accepted because they are the
/// names people reach for, and a credential channel that answers to one spelling
/// only fails in a way that reads as a bad token.
pub fn env_token() -> Option<String> {
    TOKEN_VARS.iter().find_map(|key| non_empty(key))
}

/// The instance, when the environment supplies one.
pub fn env_server_url() -> Option<String> {
    non_empty("NUAGE_SERVER_URL")
}

/// The space override from the environment, when one is set.
///
/// A raw value rather than an id: `NUAGE_SPACE` accepts a name as well, and
/// resolving a name costs a round-trip, which this module has no business
/// making.
pub fn env_space() -> Option<String> {
    non_empty("NUAGE_SPACE")
}

fn non_empty(key: &str) -> Option<String> {
    let value = std::env::var(key).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}
