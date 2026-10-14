//! The enrolment token (GOAL v0.4 section C, design slice 3): the host's Noise static
//! key plus a pre-shared key, base64 under a versioned prefix, printed once by the
//! session host and stored once by the worker. The token is what a worker needs to
//! authenticate the host (static key) and to make a host accept it (the PSK mixed into
//! every handshake) — it is a credential, so the CLI stores it at mode 0600 and prints
//! it only to the operator who asked for it.

use crate::chan::PARAMS;

/// Token prefix and version: `fieldsmith1.<base64(64 bytes)>`.
pub const PREFIX: &str = "fieldsmith1.";

/// 32-byte X25519 static key / PSK.
type Key = [u8; 32];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    /// The host's Noise static public key: the worker pins the host against it.
    pub host_static: Key,
    /// The pre-shared key, mixed into handshake messages 2 and 3 (`XXpsk3`).
    pub psk: Key,
}

impl Token {
    /// A fresh host credential: the Noise static keypair plus a PSK. Both come from
    /// snow's own key generation (this crate imports no separate RNG): one keypair is
    /// the static identity, the second's private scalar (a CSPRNG random, X25519-
    /// clamped) is the PSK.
    pub fn generate() -> (Self, Key) {
        let (host_priv, host_pub) = static_keypair();
        let psk = static_keypair().0;
        (Token { host_static: host_pub, psk }, host_priv)
    }

    pub fn encode(&self) -> String {
        let mut b = [0u8; 64];
        b[..32].copy_from_slice(&self.host_static);
        b[32..].copy_from_slice(&self.psk);
        format!("{PREFIX}{}", fieldsmith_api::b64::encode(&b))
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        let rest = s
            .strip_prefix(PREFIX)
            .ok_or_else(|| format!("token: expected the {PREFIX}… form"))?;
        let b = fieldsmith_api::b64::decode(rest).map_err(|e| format!("token: base64: {e}"))?;
        if b.len() != 64 {
            return Err(format!(
                "token: expected 64 bytes after the prefix (host key + pre-shared key), got {}",
                b.len()
            ));
        }
        let mut host_static = [0u8; 32];
        let mut psk = [0u8; 32];
        host_static.copy_from_slice(&b[..32]);
        psk.copy_from_slice(&b[32..]);
        Ok(Token { host_static, psk })
    }
}

/// A fresh static keypair through snow, `(private, public)`. The host's identity and
/// the worker's per-connection identity both come from here.
pub fn static_keypair() -> (Key, Key) {
    let params: snow::params::NoiseParams = PARAMS
        .parse()
        .expect("the Noise parameter string is a constant with known-good syntax");
    let kp = snow::Builder::new(params)
        .generate_keypair()
        .expect("snow generates a static keypair without extra configuration");
    (into32(&kp.private), into32(&kp.public))
}

fn into32(b: &[u8]) -> Key {
    let mut k = [0u8; 32];
    assert_eq!(b.len(), 32, "X25519 key material is 32 bytes");
    k.copy_from_slice(b);
    k
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_roundtrips_and_never_repeats() {
        let (t1, priv1) = Token::generate();
        let t1b: Token = Token::parse(&t1.encode()).expect("the encoded form parses back");
        assert_eq!(t1, t1b);
        assert_ne!(t1.host_static, priv1, "the public half is not the private half");
        assert_ne!(t1.host_static, [0u8; 32]);
        assert_ne!(t1.psk, [0u8; 32]);

        let (t2, _) = Token::generate();
        assert_ne!(t1, t2, "two generations are two distinct credentials");

        let bad = "fieldsmith2.".to_string()
            + &"A".repeat(88)
            + " shorter than 64 bytes";
        assert!(Token::parse(&bad).is_err());
        assert!(Token::parse("not a token").is_err());
        // 88 chars is exactly the base64 of 64 bytes; a wrong-content token still parses
        // if it is the right length — the wire protocol, not the parser, rejects it.
    }
}