use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::io::{self, IsTerminal, Write};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::api::ApiClient;
use crate::config::{self, Config};
use crate::ui;

/// How long to wait for the browser to come back before giving up. Long enough
/// to type a password and answer a second factor, short enough that a closed
/// tab does not leave a listener open forever.
const WAIT: Duration = Duration::from_secs(180);

/// What a fresh install starts with. Existing configs keep whatever the user
/// put there — a login has no business editing an ignore list.
const DEFAULT_IGNORE: [&str; 5] = [".DS_Store", "*.tmp", ".nuage/", "Thumbs.db", ".git/"];

const DEFAULT_SYNC_DIR: &str = "~/Nuage";

#[derive(Deserialize)]
struct ExchangeResponse {
    token: String,
}

/// What the server says it will accept. Absent or unreadable, we assume the
/// oldest shape — a Nuage that predates porte answers 404 here and still takes
/// an API token.
#[derive(Deserialize, Default)]
struct AuthConfig {
    #[serde(default)]
    sso_only: bool,
    #[serde(default)]
    oidc_enabled: bool,
}

/// Signs in and writes the credential into `~/.nuage.yml`, preserving every
/// field the login does not own.
///
/// The browser flow is the default because the suite runs `SSO_ONLY=true`: the
/// CLI never sees the identity provider, never handles a password, and never
/// holds an authorization code that is worth anything on its own. It opens a
/// loopback port, sends the browser to the API with that port and a nonce
/// attached, and the API — after the provider has done its part — redirects
/// back with a one-time code valid for sixty seconds and usable once.
///
/// `force_token` and a machine with no browser both fall back to pasting an API
/// token, because a headless box still needs a way in.
pub async fn run(server: Option<String>, force_token: bool) -> Result<()> {
    let fresh = !Config::path()?.exists();
    let server = resolve_server(server)?;
    let api = api_base(&server);

    let auth = discover(&api).await;

    let token = if force_token || !auth.oidc_enabled {
        if auth.sso_only && !force_token {
            bail!("this instance accepts single sign-on only but did not advertise it — run `nuage login --token` with an API token from the dashboard");
        }
        prompt_token()?
    } else {
        match sso(&api).await {
            Ok(token) => token,
            Err(err) if !auth.sso_only && io::stdin().is_terminal() => {
                ui::warn(&format!("{err:#}"));
                ui::hint("Falling back to an API token.");
                prompt_token()?
            }
            Err(err) => return Err(err),
        }
    };

    let mut config = Config::load_or_default()?;
    config.server_url = api.clone();
    config.token = token;
    if fresh {
        config.sync_dir = ask_sync_dir()?;
        config.ignore_patterns = DEFAULT_IGNORE.iter().map(|p| p.to_string()).collect();
    }

    ui::step("Testing the connection");
    let client = ApiClient::new(&config.server_url, &config.token)?;
    client.test_connection().await?;

    config.save()?;
    ui::success(&format!(
        "Signed in, saved to {}",
        Config::path()?.display()
    ));

    let sync_path = config.sync_dir_expanded()?;
    std::fs::create_dir_all(&sync_path)
        .with_context(|| format!("cannot create the sync directory {}", sync_path.display()))?;
    ui::success(&format!("Sync directory ready at {}", sync_path.display()));
    ui::hint("Run `nuage start` to sync in the background, or `nuage watch` in the foreground.");
    Ok(())
}

/// Blanks the token and leaves everything else alone.
///
/// Running it while already logged out is not an error: the state that makes
/// someone log out is often the state where they are unsure they are logged in.
pub fn logout() -> Result<()> {
    let mut config = Config::load_or_default()?;
    if config.token.is_empty() {
        ui::success("Already signed out");
        return Ok(());
    }

    config.token.clear();
    config.save()?;
    ui::success(&format!(
        "Signed out, token cleared from {}",
        Config::path()?.display()
    ));
    if config::env_token().is_some() {
        ui::warn("NUAGE_TOKEN is still set in this environment and overrides the config file");
    }
    Ok(())
}

/// Runs the loopback handoff and returns the session token.
async fn sso(api: &str) -> Result<String> {
    // Port zero asks the kernel for a free one, so two shells can log in at the
    // same time without agreeing on anything.
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("cannot open a loopback port to receive the login")?;
    let port = listener.local_addr()?.port();

    // The nonce is what makes the listener able to tell its own callback from
    // one somebody else sent. Without it any local process that guesses the
    // port can hand us a code of its choosing and we would exchange it.
    let state = nonce();

    let url = format!("{api}/auth/oidc?flow=cli&port={port}&cli_state={state}");
    ui::step("Opening the browser to sign in");
    ui::hint(&url);
    if open_browser(&url).is_err() {
        bail!("could not open a browser — paste that URL into one, or run `nuage login --token`");
    }

    let code = match tokio::time::timeout(WAIT, wait_for_code(listener, &state)).await {
        Ok(result) => result?,
        Err(_) => bail!("timed out waiting for the browser — run `nuage login` again"),
    };

    exchange(api, &code).await
}

/// Asks the server which flows it accepts, rather than asking the human what
/// their instance is configured for.
///
/// A server too old to answer is not a reason to refuse: it predates porte, so
/// it takes an API token and nothing else.
async fn discover(api: &str) -> AuthConfig {
    let url = format!("{api}/auth/config");
    let Ok(response) = reqwest::Client::new().get(&url).send().await else {
        return AuthConfig::default();
    };
    if !response.status().is_success() {
        return AuthConfig::default();
    }
    response.json().await.unwrap_or_default()
}

fn prompt_token() -> Result<String> {
    if !io::stdin().is_terminal() {
        bail!("no terminal to read an API token from — set NUAGE_TOKEN instead");
    }
    ui::step("Paste an API token from the Nuage dashboard, under Settings then API");
    print!("API token: ");
    io::stdout().flush()?;
    let token = rpassword::read_password().context("cannot read the API token")?;
    let token = token.trim().to_string();
    if token.is_empty() {
        bail!("the API token was empty — mint one in the dashboard under Settings then API");
    }
    Ok(token)
}

fn ask_sync_dir() -> Result<String> {
    if !io::stdin().is_terminal() {
        return Ok(DEFAULT_SYNC_DIR.to_string());
    }
    print!("Sync directory [{DEFAULT_SYNC_DIR}]: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(DEFAULT_SYNC_DIR.to_string());
    }
    Ok(trimmed.to_string())
}

/// api_base is the value the rest of the CLI expects in `server_url`: the API
/// root, including `/api`.
///
/// Every request is built by appending a path to it verbatim — `ApiClient` uses
/// a bare `format!` — so a `server_url` of `https://nuage.example.com` silently
/// produces `https://nuage.example.com/sync/state`, which the SPA answers with
/// its own index page. Accepting the bare host on the command line and
/// normalising here is what makes `--server https://nuage.example.com` do the
/// obvious thing.
fn api_base(server: &str) -> String {
    let trimmed = server.trim_end_matches('/');
    if trimmed.ends_with("/api") {
        return trimmed.to_string();
    }
    format!("{trimmed}/api")
}

fn resolve_server(server: Option<String>) -> Result<String> {
    if let Some(server) = server {
        return Ok(server);
    }
    if let Some(url) = config::env_server_url() {
        return Ok(url);
    }
    if let Ok(existing) = Config::load_or_default() {
        if !existing.server_url.is_empty() {
            return Ok(existing.server_url);
        }
    }
    if io::stdin().is_terminal() {
        print!("Server URL: ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let trimmed = input.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    bail!("no server known — run `nuage login --server https://nuage.example.com`")
}

/// wait_for_code serves exactly one request: the redirect the API sends the
/// browser to. It parses the request line rather than pulling in an HTTP
/// server, because the only request it will ever see is a GET whose URL it
/// constructed itself.
async fn wait_for_code(listener: TcpListener, expected_state: &str) -> Result<String> {
    loop {
        let (mut stream, _) = listener.accept().await?;

        let mut buffer = [0u8; 2048];
        let read = stream.read(&mut buffer).await?;
        let request = String::from_utf8_lossy(&buffer[..read]);
        let target = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap_or("");

        // A browser asks for /favicon.ico unprompted; answering it as if it
        // were the callback would fail the login for no reason.
        let Some(code) = query_value(target, "code") else {
            respond(&mut stream, "404 Not Found", "Not the login redirect.").await?;
            continue;
        };

        // A callback carrying a code but the wrong nonce is not noise, it is
        // somebody else's. Aborting is the point of sending one.
        if query_value(target, "state").as_deref() != Some(expected_state) {
            respond(
                &mut stream,
                "400 Bad Request",
                "The callback did not match this login attempt. Run `nuage login` again.",
            )
            .await?;
            bail!(
                "the sign-in callback did not match this login attempt — run `nuage login` again"
            );
        }

        respond(
            &mut stream,
            "200 OK",
            "Signed in. You can close this tab and go back to your terminal.",
        )
        .await?;
        return Ok(code);
    }
}

/// A nonce the server echoes back, so the listener can recognise its own
/// callback. `/dev/urandom` keeps this free of a dependency; the fallback only
/// ever runs where that file is missing, and a guessable nonce still beats none.
fn nonce() -> String {
    use std::io::Read;

    let mut bytes = [0u8; 16];
    let seeded = std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .is_ok();

    if !seeded {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        let mixed = nanos ^ ((std::process::id() as u128) << 96);
        bytes.copy_from_slice(&mixed.to_le_bytes());
    }

    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn query_value(target: &str, key: &str) -> Option<String> {
    let query = target.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        if name != key || value.is_empty() {
            return None;
        }
        Some(percent_decode(value))
    })
}

/// percent_decode handles what a one-time code can actually contain. porte's
/// codes are base64url, so nothing needs escaping — but the value arrives
/// through a URL and assuming it is clean is how the one code with a `+` in it
/// fails a year from now.
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    Err(_) => {
                        out.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

async fn respond(stream: &mut tokio::net::TcpStream, status: &str, message: &str) -> Result<()> {
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Nuage</title>\
         <body style=\"font:16px/1.5 system-ui,sans-serif;margin:4rem auto;max-width:32rem;padding:0 1rem\">\
         <h1>Nuage</h1><p>{message}</p>"
    );
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

async fn exchange(api: &str, code: &str) -> Result<String> {
    let url = format!("{api}/auth/oidc/exchange");
    let response = reqwest::Client::new()
        .post(&url)
        .json(&serde_json::json!({ "code": code }))
        .send()
        .await
        .context("cannot reach the server to exchange the login code")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        bail!("the server refused the login code ({status}): {body}");
    }
    let exchanged: ExchangeResponse = response
        .json()
        .await
        .context("the server's answer to the code exchange was not what was expected")?;
    if exchanged.token.is_empty() {
        bail!("the server returned an empty token — run `nuage login` again");
    }
    Ok(exchanged.token)
}

fn open_browser(url: &str) -> Result<()> {
    let command = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    let status = std::process::Command::new(command)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if !status.success() {
        bail!("browser command failed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // server_url is appended to verbatim by ApiClient, so it has to be the API
    // root. Getting this wrong sends every later request to the SPA, and the
    // login itself would still appear to succeed.
    #[test]
    fn api_base_normalises_what_a_human_would_type() {
        for input in [
            "https://nuage.facile.studio",
            "https://nuage.facile.studio/",
            "https://nuage.facile.studio/api",
            "https://nuage.facile.studio/api/",
        ] {
            assert_eq!(
                api_base(input),
                "https://nuage.facile.studio/api",
                "{input}"
            );
        }
    }

    #[test]
    fn the_code_is_read_from_the_redirect_and_nothing_else_is() {
        assert_eq!(
            query_value("/?code=abc123", "code").as_deref(),
            Some("abc123")
        );
        assert_eq!(
            query_value("/?state=x&code=abc123", "code").as_deref(),
            Some("abc123")
        );
        assert_eq!(query_value("/favicon.ico", "code"), None);
        assert_eq!(query_value("/?code=", "code"), None);
        assert_eq!(query_value("/", "code"), None);
    }

    #[test]
    fn percent_decoding_survives_an_escaped_code() {
        assert_eq!(percent_decode("a-b_c"), "a-b_c");
        assert_eq!(percent_decode("a%2Bb"), "a+b");
        assert_eq!(percent_decode("a+b"), "a b");
    }

    #[test]
    fn nonce_is_hex_and_does_not_repeat() {
        let first = nonce();
        assert_eq!(first.len(), 32);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(first, nonce());
    }

    #[tokio::test]
    async fn a_callback_with_the_wrong_nonce_is_refused() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            stream
                .write_all(b"GET /?code=stolen&state=not-ours HTTP/1.1\r\n\r\n")
                .await
                .unwrap();
            let mut sink = Vec::new();
            let _ = stream.read_to_end(&mut sink).await;
        });

        assert!(wait_for_code(listener, "ours").await.is_err());
    }

    #[tokio::test]
    async fn a_stray_request_does_not_fail_the_login() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            stream
                .write_all(b"GET /favicon.ico HTTP/1.1\r\n\r\n")
                .await
                .unwrap();
            let mut sink = Vec::new();
            let _ = stream.read_to_end(&mut sink).await;

            let mut second = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            second
                .write_all(b"GET /?code=real&state=ours HTTP/1.1\r\n\r\n")
                .await
                .unwrap();
            let mut sink = Vec::new();
            let _ = second.read_to_end(&mut sink).await;
        });

        assert_eq!(wait_for_code(listener, "ours").await.unwrap(), "real");
    }
}
