//! RFC 4648 base64 (standard alphabet, canonical padding) for the two payloads the API
//! carries: `.fsmz` bytes in a request (`docz`) and enrolment tokens on the worker
//! side. Hand-rolled to keep the crate's dependency surface at the serde pair; the
//! decode is strict (padding required, whitespace allowed between groups, non-canonical
//! trailing bits rejected) and tested against known vectors.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn value(b: u8) -> Option<u8> {
    match b {
        b'A'..=b'Z' => Some(b - b'A'),
        b'a'..=b'z' => Some(b - b'a' + 26),
        b'0'..=b'9' => Some(b - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Decode standard base64 with padding. Newlines, spaces and tabs between groups are
/// tolerated (a request may pretty-print); anything else that is not an alphabet byte
/// is an error naming the offset.
pub fn decode(s: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut group: [u8; 4] = [0; 4];
    let mut g = 0usize;
    let mut pad = 0usize;
    for (i, &c) in s.as_bytes().iter().enumerate() {
        if c.is_ascii_whitespace() {
            continue;
        }
        if c == b'=' {
            pad += 1;
            group[g] = 0;
            g += 1;
            if pad > 2 {
                return Err(format!("base64: too much padding at byte {i}"));
            }
        } else {
            if pad > 0 {
                return Err(format!("base64: data after padding at byte {i}"));
            }
            let v = value(c).ok_or_else(|| format!("base64: bad byte {c:#04x} at {i}"))?;
            group[g] = v;
            g += 1;
        }
        // The flush is shared: a padded final group reaches g == 4 through the `=`
        // branch too.
        if g == 4 {
            let n = (group[0] as u32) << 18
                | (group[1] as u32) << 12
                | (group[2] as u32) << 6
                | group[3] as u32;
            out.push((n >> 16) as u8);
            out.push((n >> 8) as u8);
            out.push(n as u8);
            // Non-canonical trailing bits on the last group are a corruption signal
            // (RFC 4648 recommends rejecting them). With one pad, the 2 unused bits are
            // the top two of out[len-1] (a `==`-like "Zm9=" must fail); with two, the 4
            // unused bits are the top nibble of out[len-2] ("Zh==" must fail). The low
            // parts of those bytes are the padding sextet itself and are always zero.
            if pad > 0 {
                let len = out.len();
                let bad = if pad == 2 {
                    out[len - 2] & 0xF0 != 0 || out[len - 1] != 0
                } else {
                    out[len - 1] & 0xC0 != 0
                };
                if bad {
                    return Err("base64: non-canonical trailing bits".into());
                }
                out.truncate(len - pad);
            }
            g = 0;
        }
    }
    if g != 0 {
        return Err(format!(
            "base64: {g} trailing digit(s) — input length mod 4 must be 0 (with padding)"
        ));
    }
    Ok(out)
}

/// Encode to canonically padded standard base64 (no line wrapping).
pub fn encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc_vectors() {
        // RFC 4648 test vectors.
        for (plain, b64) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(encode(plain.as_bytes()), b64, "encode {plain:?}");
            assert_eq!(decode(b64).unwrap(), plain.as_bytes(), "decode {b64}");
        }
    }

    #[test]
    fn round_trips_zip_like_bytes() {
        // The docz payload is an arbitrary binary zip — a spread of all 256 values
        // round-trips exactly.
        let data: Vec<u8> = (0..4096u32).map(|i| (i * 37 % 256) as u8).collect();
        assert_eq!(decode(&encode(&data)).unwrap(), data);
    }

    #[test]
    fn whitespace_between_groups_is_tolerated() {
        let broken = "Zm9v\nYmFy"; // "foobar" wrapped
        assert_eq!(decode(broken).unwrap(), b"foobar");
    }

    #[test]
    fn rejects_the_corrupt_cases() {
        assert!(decode("Zg=").is_err()); // wrong length
        assert!(decode("Z===").is_err()); // too much padding
        assert!(decode("Zh==").is_err()); // non-canonical trailing bits (h≠pad)
        assert!(decode("Zm9=").is_err()); // non-canonical trailing bits, one pad
        assert!(decode("Zm9!").is_err()); // bad byte
        assert!(decode("Zm9v====").is_err()); // data shape garbage
    }
}
