use anyhow::Result;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::handoff;

use super::query::query_value;

/// wait_for_code serves the redirect the API sends the browser to, and keeps
/// listening through everything else. It parses the request line rather than
/// pulling in an HTTP server, because the only request that matters is a GET
/// whose URL it constructed itself.
///
/// Two kinds of request are answered and ignored, and the loop is what makes
/// them harmless. A browser asks for `/favicon.ico` without being told to, so
/// ending the login there produces a failure with nothing in it a person could
/// act on. A callback carrying a code under the wrong nonce is the other half
/// of the same rule: it must be refused, because any page the user has open can
/// hit `http://127.0.0.1:<port>/?code=…` and hand this CLI a session that is not
/// the user's, and it must not end the login either, because that would let the
/// same page close a login it did not start. Both refusals leave the listener
/// waiting for the callback that does belong to it, which is what the page they
/// serve says.
///
/// The nonce is compared exactly. A constant-time comparison buys nothing here:
/// the value is checked once per request against a listener that is not
/// reachable off this machine, and a timing oracle needs an attacker who can
/// already run code on it.
pub(super) async fn wait_for_code(listener: TcpListener, expected_state: &str) -> Result<String> {
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

        let Some(code) = query_value(target, "code") else {
            respond(&mut stream, "404 Not Found", &handoff::NO_CODE).await?;
            continue;
        };

        if query_value(target, "state").as_deref() != Some(expected_state) {
            respond(&mut stream, "400 Bad Request", &handoff::NOT_THIS_LOGIN).await?;
            continue;
        }

        respond(&mut stream, "200 OK", &handoff::SIGNED_IN).await?;
        return Ok(code);
    }
}

/// A nonce the server echoes back, so the listener can recognise its own
/// callback. `/dev/urandom` keeps this free of a dependency; the fallback only
/// ever runs where that file is missing, and a guessable nonce still beats none.
pub(super) fn nonce() -> String {
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

/// respond writes one page and flushes it before the stream is dropped, so the
/// browser finishes reading the page rather than being handed a reset
/// connection at the moment the login succeeds.
///
/// Every answer is the same page. The old success and failure pages agreed only
/// by accident, having been written minutes apart, and a refusal that looks
/// like a different application is a refusal a person reads as a broken login
/// rather than as one that is still waiting for them.
pub(super) async fn respond(
    stream: &mut tokio::net::TcpStream,
    status: &str,
    page: &handoff::Page,
) -> Result<()> {
    let body = page.render();
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonce_is_hex_and_does_not_repeat() {
        let first = nonce();
        assert_eq!(first.len(), 32);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(first, nonce());
    }

    // A callback carrying a code under the wrong nonce is somebody else's, and
    // both halves of the answer matter. Accepting it would hand this CLI a
    // session that is not the user's. Ending the login over it, which is what
    // this used to do, would let any page the user has open close a login it
    // did not start.
    #[tokio::test]
    async fn a_callback_with_the_wrong_nonce_is_refused_and_the_login_stays_open() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let refusal = tokio::spawn(async move {
            let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            stream
                .write_all(b"GET /?code=stolen&state=not-ours HTTP/1.1\r\n\r\n")
                .await
                .unwrap();
            let mut answer = Vec::new();
            let _ = stream.read_to_end(&mut answer).await;

            let mut second = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            second
                .write_all(b"GET /?code=real&state=ours HTTP/1.1\r\n\r\n")
                .await
                .unwrap();
            let mut sink = Vec::new();
            let _ = second.read_to_end(&mut sink).await;
            String::from_utf8_lossy(&answer).into_owned()
        });

        assert_eq!(wait_for_code(listener, "ours").await.unwrap(), "real");

        let answer = refusal.await.unwrap();
        assert!(answer.starts_with("HTTP/1.1 400 "), "{answer}");
        assert!(
            !answer.contains("stolen"),
            "the refusal page echoes the code it refused"
        );
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
