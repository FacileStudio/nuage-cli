use super::sample;

// A config that names a command and still carries a token means the plaintext
// value is the leftover, so the command's output has to replace it rather than
// lose to it.
#[test]
fn key_command_output_replaces_the_stored_token() {
    let mut config = sample();
    config.token = "stale-plaintext".to_string();
    config.key_command = "printf 'from-command\\n'".to_string();

    config.apply_key_command().unwrap();

    assert_eq!(config.token, "from-command");
}

// The failure has to name the command. An empty token would reach validate() and
// report "not signed in", which sends the user to `nuage login` for a broken
// command.
#[test]
fn a_failing_key_command_reports_the_command() {
    let mut config = sample();
    config.key_command = "exit 3".to_string();

    let err = config.apply_key_command().unwrap_err().to_string();

    assert!(err.contains("key_command"), "{err}");
    assert!(err.contains("exit 3"), "{err}");
}

#[test]
fn a_silent_key_command_is_an_error_not_an_empty_token() {
    let mut config = sample();
    config.key_command = "true".to_string();

    assert!(config.apply_key_command().is_err());
}

#[test]
fn no_key_command_leaves_the_token_alone() {
    let mut config = sample();
    config.token = "kept".to_string();

    config.apply_key_command().unwrap();

    assert_eq!(config.token, "kept");
}
