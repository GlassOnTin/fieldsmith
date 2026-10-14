//! The session host (design slice 3): `fieldsmith session` binds where the user says
//! (default `127.0.0.1`), generates its credential — the Noise static keypair whose
//! public half is the token's host key, plus the pre-shared key — and serves jobs.
//!
//! One listener, one loop: accept until a connection completes the Noise handshake,
//! send the job, receive the answer, drop the channel. Timing starts at accept, so the
//! host wall covers handshake + job transfer + the worker's compute + the answer back —
//! the number the costs row and the overhead comparison are made of (the worker's own
//! per-rung walls travel inside the envelope, so transit is the difference, measured
//! per job).

use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Instant;

use crate::chan::{Chan, ChanError};
use crate::token::{static_keypair, Token};

pub struct Host {
    listener: TcpListener,
    static_priv: [u8; 32],
    token: Token,
}

/// What a served job produced: the envelope verbatim and the host-measured wall.
pub struct HostOutcome {
    pub response: String,
    pub wall_ms: f64,
}

impl Host {
    /// Bind and mint the credential. `bind` is a std listener spec — `"127.0.0.1"`,
    /// `"0.0.0.0"`, an address literal; the port is appended (`":0"` = ephemeral).
    pub fn bind(bind: &str, port: u16) -> Result<Host, String> {
        let listener = TcpListener::bind((bind, port))
            .map_err(|e| format!("session: cannot bind {bind}:{port}: {e}"))?;
        let (token, static_priv) = Token::generate();
        Ok(Host {
            listener,
            static_priv,
            token,
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr, String> {
        self.listener
            .local_addr()
            .map_err(|e| format!("session: local address unavailable: {e}"))
    }

    /// The enrolment token: the host's Noise static key + pre-shared key, base64. A
    /// credential — the caller prints it once to the operator.
    pub fn token(&self) -> &Token {
        &self.token
    }

    /// Wait for the next worker connection and run one job through it: outbound job
    /// request, inbound answer. Timing spans accept through answer.
    pub fn exchange(
        &self,
        request: &str,
        log: &mut dyn FnMut(&str),
    ) -> Result<HostOutcome, String> {
        let start = Instant::now();
        let mut chan = self.accept_channel(log)?;
        chan.send(request.as_bytes())
            .map_err(|e| format!("session: sending the job to the worker failed: {e}"))?;
        let response = chan
            .recv()
            .map_err(|e| format!("session: receiving the answer failed: {e}"))?;
        let wall_ms = start.elapsed().as_secs_f64() * 1000.0;
        let response = String::from_utf8(response)
            .map_err(|_| "session: the answer was not UTF-8 — refusing".to_string())?;
        Ok(HostOutcome { response, wall_ms })
    }

    /// Accept until a connection handshakes as a worker. Failed attempts — a port
    /// scanner sending garbage, or a worker whose PSK does not match (snow applies
    /// psk3 inside the responder's read of message 3, so the mismatch fails here with
    /// a decrypt error) — are reported and dropped, not fatal; the loop moves on to
    /// the next accept.
    fn accept_channel(&self, log: &mut dyn FnMut(&str)) -> Result<Chan, String> {
        loop {
            let (stream, peer) = self
                .listener
                .accept()
                .map_err(|e| format!("session: accept failed: {e}"))?;
            match self.handshake(stream) {
                Ok(chan) => return Ok(chan),
                Err(e) => {
                    log(&format!("session: rejected {peer}: {e}"));
                }
            }
        }
    }

    fn handshake(&self, stream: TcpStream) -> Result<Chan, ChanError> {
        Chan::responder(stream, &self.static_priv, &self.token.psk)
    }
}

/// The worker's own Noise static keypair (a fresh identity per job is fine — the worker
/// authenticates the host against the token; the host trusts only the PSK side).
pub fn worker_static_keypair() -> ([u8; 32], [u8; 32]) {
    static_keypair()
}
