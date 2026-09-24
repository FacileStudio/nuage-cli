pub(super) fn query_value(target: &str, key: &str) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
