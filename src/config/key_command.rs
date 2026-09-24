use anyhow::{bail, Context, Result};
use std::process::Command;

/// Runs one command through `sh -c` and returns its trimmed standard output.
///
/// A shell rather than a split argv, because the useful commands are pipelines
/// and subcommands (`casier get nuage`, `op read op://vault/nuage/token`) that an
/// argv split cannot express.
///
/// A failure names the command and nothing else. Neither its standard error nor
/// its standard output is echoed, because a command that fails while holding the
/// credential can put it in either.
pub(super) fn run_key_command(command: &str) -> Result<String> {
    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .output()
        .with_context(|| format!("cannot run key_command `{command}`"))?;

    if !output.status.success() {
        bail!("key_command `{command}` failed: {}", output.status);
    }

    let stdout = String::from_utf8(output.stdout)
        .with_context(|| format!("key_command `{command}` printed invalid utf-8"))?;
    let token = stdout.trim();

    if token.is_empty() {
        bail!("key_command `{command}` printed nothing");
    }
    Ok(token.to_string())
}
