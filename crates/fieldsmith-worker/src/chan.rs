//! The noise-framed channel: one [`Chan`] per TCP connection, `Noise_XXpsk3_25519_-
//! ChaChaPoly_SHA256` through the `snow` crate (hand-rolled crypto is off the table —
//! GOAL design slice 3). Two rules shape the wire:
//!
//! - Every peer-to-peer unit — handshake message or ciphertext frame — is
//!   `[u32 BE length][bytes]`, so the reader never guesses.
//! - An application payload larger than one Noise transport message (snow caps each at
//!   65535 bytes with the 16-byte tag) travels as chunks inside one logical message: the
//!   first decrypted chunk carries `[u64 BE total]`, the reader reassembles until it has
//!   that many payload bytes. Job requests and envelopes are single logical messages.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

pub const PARAMS: &str = "Noise_XXpsk3_25519_ChaChaPoly_SHA256";

/// snow's transport-message cap (ChaChaPoly's 16-byte tag included).
const MAX_MSG: usize = 65535;
const TAG: usize = 16;
/// Largest plaintext chunk the sender encrypts per transport message.
const PLAIN_CHUNK: usize = MAX_MSG - TAG;
/// Recv-side payload cap (a job request is a question, an answer a small JSON envelope;
/// the cap is protocol sanity, not a real limit).
const MAX_PAYLOAD: usize = 512 << 20;
/// Socket stall guard: any single read or write must finish inside this window, so a
/// peer that vanishes mid-handshake or mid-transfer errors the channel instead of
/// blocking its holder forever (the empty-listener hang that once wedged the test
/// harness). Per read call, not per logical message — a slow link that keeps bytes
/// flowing is unaffected, and compute-heavy solves never read the socket at all.
/// Not covered by a unit test: a stalled peer needs a thread to hold a socket still
/// for a minute.
pub const STALL: Duration = Duration::from_secs(60);

#[derive(Debug)]
pub enum ChanError {
    Io(std::io::Error),
    Noise(snow::Error),
    Protocol(String),
}

impl std::fmt::Display for ChanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChanError::Io(e) => write!(f, "i/o: {e}"),
            ChanError::Noise(e) => write!(f, "noise: {e}"),
            ChanError::Protocol(m) => write!(f, "protocol: {m}"),
        }
    }
}

impl std::error::Error for ChanError {}

pub struct Chan {
    stream: TcpStream,
    noise: snow::TransportState,
}

impl Chan {
    /// The worker side: dial a stream, handshake as initiator. XX gives the initiator
    /// no pre-known peer static key, so the token's host key pins the host *after* the
    /// exchange — a host whose msg-2 static does not match the token is refused here,
    /// before any application byte crosses.
    pub fn initiator(
        mut stream: TcpStream,
        own_static_priv: &[u8; 32],
        host_static: &[u8; 32],
        psk: &[u8; 32],
    ) -> Result<Self, ChanError> {
        set_stream_options(&mut stream)?;
        let mut noise = builder()?
            .local_private_key(own_static_priv)
            .and_then(|b| b.psk(3, psk))
            .and_then(|b| b.build_initiator())
            .map_err(ChanError::Noise)?;
        hs_write(&mut noise, &mut stream, &[])?; // msg 1: -> e
        let m2 = hs_read(&mut noise, &mut stream)?; // msg 2: <- es, ss, s (+ psk)
        expect_empty(&m2)?;
        hs_write(&mut noise, &mut stream, &[])?; // msg 3: -> s (+ psk)
        let noise = noise.into_transport_mode().map_err(ChanError::Noise)?;
        let pinned = noise.get_remote_static().ok_or_else(|| {
            ChanError::Protocol("the handshake carried no host static key".into())
        })?;
        if pinned != host_static {
            return Err(ChanError::Protocol(
                "the host's Noise static key does not match the token — refusing".into(),
            ));
        }
        Ok(Chan { stream, noise })
    }

    /// The host side: accept a stream, handshake as responder. Its only credential is
    /// the static private key whose public half rides in the token, plus the PSK both
    /// sides mix in.
    pub fn responder(
        mut stream: TcpStream,
        own_static_priv: &[u8; 32],
        psk: &[u8; 32],
    ) -> Result<Self, ChanError> {
        set_stream_options(&mut stream)?;
        let mut noise = builder()?
            .local_private_key(own_static_priv)
            .and_then(|b| b.psk(3, psk))
            .and_then(|b| b.build_responder())
            .map_err(ChanError::Noise)?;
        let m1 = hs_read(&mut noise, &mut stream)?; // msg 1
        expect_empty(&m1)?;
        hs_write(&mut noise, &mut stream, &[])?; // msg 2
        let m3 = hs_read(&mut noise, &mut stream)?; // msg 3
        expect_empty(&m3)?;
        Ok(Chan {
            stream,
            noise: noise.into_transport_mode().map_err(ChanError::Noise)?,
        })
    }

    /// One logical message out: chunked, ciphertext-framed.
    pub fn send(&mut self, payload: &[u8]) -> Result<(), ChanError> {
        if payload.len() > MAX_PAYLOAD {
            return Err(ChanError::Protocol(format!(
                "payload {} bytes exceeds the {}-byte protocol cap",
                payload.len(),
                MAX_PAYLOAD
            )));
        }
        let total = payload.len();
        let mut sent = 0usize;
        let mut first = true;
        loop {
            let reserve = if first { 8 } else { 0 };
            let take = (PLAIN_CHUNK - reserve).min(total - sent);
            let mut plain = Vec::with_capacity(reserve + take);
            if first {
                plain.extend_from_slice(&(total as u64).to_be_bytes());
            }
            plain.extend_from_slice(&payload[sent..sent + take]);
            sent += take;
            let mut ct = vec![0u8; plain.len() + TAG];
            let n = self
                .noise
                .write_message(&plain, &mut ct)
                .map_err(ChanError::Noise)?;
            frame_write(&mut self.stream, &ct[..n])?;
            first = false;
            if sent >= total {
                return Ok(());
            }
        }
    }

    /// One logical message in: decrypted frames reassembled to the declared length.
    pub fn recv(&mut self) -> Result<Vec<u8>, ChanError> {
        let mut acc: Vec<u8> = Vec::new();
        let mut total: Option<usize> = None;
        loop {
            let ct = frame_read(&mut self.stream)?;
            let mut plain = vec![0u8; ct.len()];
            let n = self
                .noise
                .read_message(&ct, &mut plain)
                .map_err(ChanError::Noise)?;
            plain.truncate(n);
            if total.is_none() {
                if plain.len() < 8 {
                    return Err(ChanError::Protocol(
                        "first transport chunk lacks the length header".into(),
                    ));
                }
                let mut h = [0u8; 8];
                h.copy_from_slice(&plain[..8]);
                total = Some(usize::try_from(u64::from_be_bytes(h)).map_err(|_| {
                    ChanError::Protocol("declared payload length overflows this platform".into())
                })?);
                acc.extend_from_slice(&plain[8..]);
            } else {
                acc.extend_from_slice(&plain);
            }
            let t = total.unwrap_or(0);
            if t > MAX_PAYLOAD {
                return Err(ChanError::Protocol(format!(
                    "declared payload {t} bytes exceeds the {MAX_PAYLOAD}-byte protocol cap"
                )));
            }
            if acc.len() == t {
                return Ok(acc);
            }
            if acc.len() > t {
                return Err(ChanError::Protocol(
                    "payload overran its declared length".into(),
                ));
            }
        }
    }
}

fn builder() -> Result<snow::Builder<'static>, ChanError> {
    let params: snow::params::NoiseParams = PARAMS
        .parse()
        .expect("the Noise parameter string is a constant with known-good syntax");
    Ok(snow::Builder::new(params))
}

fn set_stream_options(s: &mut TcpStream) -> Result<(), ChanError> {
    s.set_nodelay(true).map_err(ChanError::Io)?;
    s.set_read_timeout(Some(STALL)).map_err(ChanError::Io)?;
    s.set_write_timeout(Some(STALL)).map_err(ChanError::Io)
}

/// The protocol refuses handshake payloads — everything application-level travels as a
/// transport message, so an unexpected handshake payload is a wire desynchronisation.
fn expect_empty(payload: &[u8]) -> Result<(), ChanError> {
    if payload.is_empty() {
        Ok(())
    } else {
        Err(ChanError::Protocol(
            "handshake message carried a payload — refusing".into(),
        ))
    }
}

fn hs_write(
    noise: &mut snow::HandshakeState,
    stream: &mut TcpStream,
    payload: &[u8],
) -> Result<(), ChanError> {
    let mut buf = vec![0u8; MAX_MSG];
    let n = noise
        .write_message(payload, &mut buf)
        .map_err(ChanError::Noise)?;
    frame_write(stream, &buf[..n])
}

fn hs_read(noise: &mut snow::HandshakeState, stream: &mut TcpStream) -> Result<Vec<u8>, ChanError> {
    let msg = frame_read(stream)?;
    let mut buf = vec![0u8; msg.len()];
    let n = noise
        .read_message(&msg, &mut buf)
        .map_err(ChanError::Noise)?;
    buf.truncate(n);
    Ok(buf)
}

fn frame_write(stream: &mut TcpStream, msg: &[u8]) -> Result<(), ChanError> {
    let mut buf = Vec::with_capacity(4 + msg.len());
    buf.extend_from_slice(&(msg.len() as u32).to_be_bytes());
    buf.extend_from_slice(msg);
    stream.write_all(&buf).map_err(ChanError::Io)
}

fn frame_read(stream: &mut TcpStream) -> Result<Vec<u8>, ChanError> {
    let mut len = [0u8; 4];
    stream.read_exact(&mut len).map_err(ChanError::Io)?;
    let n = u32::from_be_bytes(len) as usize;
    if n > MAX_MSG {
        return Err(ChanError::Protocol(format!(
            "frame length {n} exceeds the protocol maximum {MAX_MSG}"
        )));
    }
    let mut buf = vec![0u8; n];
    stream.read_exact(&mut buf).map_err(ChanError::Io)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::Token;

    /// Drive one responder and one initiator over a loopback pair.
    fn pair() -> (Chan, Chan, Token) {
        let (token, host_priv) = Token::generate();
        let psk = token.psk;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        let handle = std::thread::spawn(move || {
            let (s, _) = listener.accept().expect("accept");
            Chan::responder(s, &host_priv, &psk)
        });
        let own_priv = static_priv();
        let s = std::net::TcpStream::connect(addr).expect("connect loopback");
        let client = Chan::initiator(s, &own_priv, &token.host_static, &token.psk)
            .expect("client handshake");
        let server = handle
            .join()
            .expect("responder thread")
            .expect("responder handshake");
        (client, server, token)
    }

    fn static_priv() -> [u8; 32] {
        let params: snow::params::NoiseParams = PARAMS.parse().expect("params");
        let kp = snow::Builder::new(params)
            .generate_keypair()
            .expect("keypair");
        let mut k = [0u8; 32];
        assert_eq!(kp.private.len(), 32);
        k.copy_from_slice(&kp.private);
        k
    }

    /// `pair()`, but the client's PSK overridden — for pinning the key-mismatch shape.
    /// Returns `(client, server_outcome)`; the responder's outcome is an `Err` under a
    /// mismatched PSK.
    fn pair_with_client_psk(client_psk: &[u8; 32]) -> (Chan, Result<Chan, ChanError>) {
        let (token, host_priv) = Token::generate();
        let host_static = token.host_static;
        let psk = token.psk;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        let handle = std::thread::spawn(move || {
            let (s, _) = listener.accept().expect("accept");
            Chan::responder(s, &host_priv, &psk)
        });
        let s = std::net::TcpStream::connect(addr).expect("connect loopback");
        let client = Chan::initiator(s, &static_priv(), &host_static, client_psk)
            .expect("the initiator's own handshake completes (see the psk test)");
        let server = handle.join().expect("responder thread");
        (client, server)
    }

    #[test]
    fn roundtrip_and_pinning() {
        let (mut client, mut server, _token) = pair();
        client.send(b"ping").expect("client send");
        assert_eq!(server.recv().expect("server recv"), b"ping");
        server.send(b"pong").expect("server send");
        assert_eq!(client.recv().expect("client recv"), b"pong");
    }

    #[test]
    fn empty_payload_is_one_header_only_frame() {
        let (mut client, mut server, _) = pair();
        client.send(b"").expect("empty send");
        assert_eq!(server.recv().expect("empty recv"), b"" as &[u8]);
    }

    #[test]
    fn payloads_past_one_noise_message_reassemble() {
        let (mut client, mut server, _) = pair();
        // A payload several transport messages wide, each chunk 65 519 bytes of
        // plaintext — well past snow's 65 535-byte ciphertext cap.
        let big: Vec<u8> = (0..200_000u32).map(|i| (i % 256) as u8).collect();
        client.send(&big).expect("chunked send");
        let got = server.recv().expect("chunked recv");
        assert_eq!(got.len(), big.len());
        assert_eq!(got, big);
    }

    #[test]
    fn wrong_psk_the_responder_rejects_msg3_and_the_initiator_gets_no_payload() {
        // Empirical shape (two earlier versions of this test pinned the wrong side:
        // the initiator is not the one that fails). snow applies psk3 inside the
        // responder's read of message 3, so a PSK-mismatched worker completes its own
        // handshake while the host rejects the session with a decrypt error — and,
        // having dropped the socket, delivers nothing: the worker's first payload
        // read ends at EOF. No job, no answer, no bytes cross.
        let (mut client, server) = pair_with_client_psk(&[3u8; 32]);
        let err = match server {
            Err(e) => e,
            Ok(_) => panic!("the host must reject a wrong-psk handshake"),
        };
        assert!(
            matches!(err, ChanError::Noise(snow::Error::Decrypt)),
            "msg3 verification fails with a decrypt error, got {err}"
        );
        let eof = client
            .recv()
            .expect_err("no payload follows a rejected handshake");
        assert!(
            matches!(eof, ChanError::Io(_)),
            "EOF at the first read, got {eof}"
        );
    }

    #[test]
    fn wrong_host_static_is_refused_after_the_exchange() {
        let (token, _) = Token::generate();
        let (_, host_priv) = Token::generate();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = std::thread::spawn(move || {
            let (s, _) = listener.accept().expect("accept");
            Chan::responder(s, &host_priv, &token.psk)
        });
        let s = std::net::TcpStream::connect(addr).expect("connect");
        let own = static_priv();
        let r = Chan::initiator(s, &own, &[9u8; 32], &token.psk);
        match r {
            Err(ChanError::Protocol(m)) => {
                assert!(m.contains("static key"), "named refusal: {m}");
            }
            Err(other) => panic!("expected a pinning refusal, got {other}"),
            Ok(_) => panic!("a non-token host must not be pinned"),
        }
        let _ = handle.join();
    }
}
