//! Embedded HTTP/1.1 compute, MCP JSON-RPC, and static Web/PWA asset server.
//!
//! Used in two places:
//! 1. **Standalone remote compute & headless workstation host** (`fieldsmith-mcp --serve 0.0.0.0:8080`
//!    or `fieldsmith serve --bind 0.0.0.0:8080 [--web-dir web/dist]`): exposes `/api/health`,
//!    `/api/workspace`, `/api/mcp`, and `/api/solve` with CORS headers so a mobile phone or
//!    remote browser can offload multi-core native FEA and coupled PDE solves over LAN,
//!    Tailscale, or WireGuard.
//! 2. **Android F-Droid local host (`sh.fieldsmith.app`)**: binds to `127.0.0.1:0` inside the
//!    Android app process and serves the bundled `web/dist` assets with
//!    `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: require-corp`
//!    headers (unlocking `SharedArrayBuffer` multi-threaded WASM as well as native `arm64-v8a`
//!    `/api/mcp` and `/api/solve` execution).

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream, UdpSocket};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fieldsmith_api::respond;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{Server, SharedWorkspaceState};

pub const DISCOVERY_UDP_PORT: u16 = 18790;
pub const DISCOVERY_MAGIC: &[u8] = b"FIELDSMITH_DISCOVER_V1";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiscoveredPeer {
    pub url: String,
    pub host: String,
    pub port: u16,
    pub hostname: String,
    pub os: String,
    pub arch: String,
    pub threads: usize,
    pub version: String,
    pub max_recommended_dofs: usize,
    pub latency_ms: u64,
    pub via: String,
}

#[derive(Clone, Debug)]
pub struct ServeConfig {
    pub bind_addr: String,
    pub workspace_path: PathBuf,
    pub web_dir: Option<PathBuf>,
}

pub struct RunningServer {
    local_addr: SocketAddr,
}

impl RunningServer {
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }
}

pub fn local_hostname() -> String {
    if let Ok(h) = std::env::var("HOSTNAME") {
        let t = h.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    if let Ok(s) = std::fs::read_to_string("/etc/hostname") {
        let t = s.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    format!("fieldsmith-{}", std::env::consts::ARCH)
}

pub fn max_recommended_dofs_for_host() -> usize {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    if std::env::consts::OS == "android" {
        40_000
    } else if threads >= 16 {
        500_000
    } else if threads >= 8 {
        250_000
    } else {
        120_000
    }
}

pub fn build_health_payload(ws: &SharedWorkspaceState, listen_port: Option<u16>) -> Value {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    json!({
        "status": "ok",
        "server": "fieldsmith-compute",
        "version": env!("CARGO_PKG_VERSION"),
        "hostname": local_hostname(),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "threads": threads,
        "listen_port": listen_port.unwrap_or(0),
        "max_recommended_dofs": max_recommended_dofs_for_host(),
        "workspace_revision": ws.revision,
        "remote_compute_url": ws.remote_compute_url,
        "capabilities": ["mcp", "workspace", "solve", "coupled_systems", "peer_discovery", "compute_offload"],
    })
}

fn spawn_udp_discovery_responder(http_port: u16, ws_path: PathBuf) {
    let Ok(sock) = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, DISCOVERY_UDP_PORT)) else {
        return;
    };
    let _ = std::thread::Builder::new()
        .name("fieldsmith-udp-beacon".to_string())
        .spawn(move || {
            let mut buf = [0u8; 512];
            while let Ok((n, src)) = sock.recv_from(&mut buf) {
                if n >= DISCOVERY_MAGIC.len() && &buf[..DISCOVERY_MAGIC.len()] == DISCOVERY_MAGIC {
                    let ws = read_workspace_file(&ws_path);
                    let reply = build_health_payload(&ws, Some(http_port));
                    if let Ok(bytes) = serde_json::to_vec(&reply) {
                        let _ = sock.send_to(&bytes, src);
                    }
                }
            }
        });
}

/// Bind `cfg.bind_addr` and spawn a background accept loop. Returns the bound [`RunningServer`]
/// (useful when binding to port `0` on Android or in tests).
pub fn spawn_http_server(cfg: ServeConfig) -> std::io::Result<RunningServer> {
    let listener = TcpListener::bind(&cfg.bind_addr)?;
    let local_addr = listener.local_addr()?;
    spawn_udp_discovery_responder(local_addr.port(), cfg.workspace_path.clone());
    let shared_cfg = Arc::new(cfg);
    let mcp_lock = Arc::new(Mutex::new(()));
    let port = local_addr.port();

    std::thread::Builder::new()
        .name("fieldsmith-http".to_string())
        .spawn(move || {
            for incoming in listener.incoming() {
                let Ok(stream) = incoming else {
                    continue;
                };
                let c = Arc::clone(&shared_cfg);
                let l = Arc::clone(&mcp_lock);
                std::thread::spawn(move || {
                    let _ = handle_connection(stream, &c, &l, port);
                });
            }
        })?;

    Ok(RunningServer { local_addr })
}

/// Run the HTTP server on the current thread (blocking).
pub fn run_http_server_blocking(cfg: ServeConfig) -> std::io::Result<()> {
    let listener = TcpListener::bind(&cfg.bind_addr)?;
    let local_addr = listener.local_addr()?;
    let port = local_addr.port();
    spawn_udp_discovery_responder(port, cfg.workspace_path.clone());
    eprintln!(
        "fieldsmith compute & MCP server listening on http://{local_addr} (hostname={}, arch={}, threads={})",
        local_hostname(),
        std::env::consts::ARCH,
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    );
    let shared_cfg = Arc::new(cfg);
    let mcp_lock = Arc::new(Mutex::new(()));
    for incoming in listener.incoming() {
        let Ok(stream) = incoming else {
            continue;
        };
        let c = Arc::clone(&shared_cfg);
        let l = Arc::clone(&mcp_lock);
        std::thread::spawn(move || {
            let _ = handle_connection(stream, &c, &l, port);
        });
    }
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn read_workspace_file(path: &Path) -> SharedWorkspaceState {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<SharedWorkspaceState>(&s).ok())
        .unwrap_or_else(|| SharedWorkspaceState {
            revision: 0,
            updated_at_ms: now_ms(),
            actor: "init".to_string(),
            last_tool: "none".to_string(),
            doc_fsm: None,
            coupled_system_id: None,
            design_p: None,
            epistemic_theta: None,
            remote_compute_url: None,
            activity_log: Vec::new(),
        })
}

fn write_workspace_file(path: &Path, ws: &SharedWorkspaceState) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(ws) {
        let _ = std::fs::write(path, text);
    }
}

pub fn parse_http_host_port(url: &str) -> Result<(String, u16, String), String> {
    let trimmed = url.trim().trim_end_matches('/');
    let rest = if let Some(r) = trimmed.strip_prefix("http://") {
        r
    } else if trimmed.starts_with("https://") {
        return Err("only http:// compute peers are supported by the zero-dep bridge".to_string());
    } else {
        trimmed
    };
    let host_port = rest.split('/').next().unwrap_or("");
    if host_port.is_empty() {
        return Err(format!("invalid peer URL: {url:?}"));
    }
    let (host, port) = if let Some((h, p_str)) = host_port.rsplit_once(':') {
        let port = p_str
            .parse::<u16>()
            .map_err(|_| format!("invalid port in URL: {url:?}"))?;
        (h.to_string(), port)
    } else {
        (host_port.to_string(), 8080)
    };
    let normalized = format!("http://{host}:{port}");
    Ok((host, port, normalized))
}

pub fn http_request_json(
    base_url: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
    timeout_ms: u64,
) -> Result<(u16, Value, u64), String> {
    use std::net::ToSocketAddrs;
    let (host, port, _) = parse_http_host_port(base_url)?;
    let t0 = Instant::now();
    let addr = format!("{host}:{port}")
        .to_socket_addrs()
        .map_err(|e| format!("resolve {host}:{port}: {e}"))?
        .find(|a| a.is_ipv4())
        .or_else(|| {
            format!("{host}:{port}")
                .to_socket_addrs()
                .ok()
                .and_then(|mut it| it.next())
        })
        .ok_or_else(|| format!("no address for {host}:{port}"))?;

    let timeout = Duration::from_millis(timeout_ms.max(50));
    let mut stream =
        TcpStream::connect_timeout(&addr, timeout).map_err(|e| format!("connect {addr}: {e}"))?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    let body_bytes = body.unwrap_or("").as_bytes();
    let req = if body.is_some() {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body_bytes.len()
        )
    } else {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n"
        )
    };
    stream
        .write_all(req.as_bytes())
        .map_err(|e| format!("write {addr}: {e}"))?;
    if !body_bytes.is_empty() {
        stream
            .write_all(body_bytes)
            .map_err(|e| format!("write body {addr}: {e}"))?;
    }
    stream.flush().ok();

    let mut raw = String::new();
    stream
        .read_to_string(&mut raw)
        .map_err(|e| format!("read {addr}: {e}"))?;
    let latency_ms = t0.elapsed().as_millis() as u64;

    let (head, body_part) = raw
        .split_once("\r\n\r\n")
        .or_else(|| raw.split_once("\n\n"))
        .ok_or_else(|| "malformed HTTP response".to_string())?;
    let status_line = head.lines().next().unwrap_or("");
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(500);
    let val: Value =
        serde_json::from_str(body_part.trim()).unwrap_or_else(|_| json!({ "raw": body_part }));
    Ok((status_code, val, latency_ms))
}

pub fn probe_peer_health(host: &str, port: u16, timeout_ms: u64, via: &str) -> Option<DiscoveredPeer> {
    let url = format!("http://{host}:{port}");
    let (code, val, latency_ms) =
        http_request_json(&url, "GET", "/api/health", None, timeout_ms).ok()?;
    if code != 200 || val.get("server").and_then(Value::as_str) != Some("fieldsmith-compute") {
        return None;
    }
    Some(DiscoveredPeer {
        url,
        host: host.to_string(),
        port,
        hostname: val
            .get("hostname")
            .and_then(Value::as_str)
            .unwrap_or(host)
            .to_string(),
        os: val
            .get("os")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        arch: val
            .get("arch")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        threads: val
            .get("threads")
            .and_then(Value::as_u64)
            .unwrap_or(1) as usize,
        version: val
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("0.0.0")
            .to_string(),
        max_recommended_dofs: val
            .get("max_recommended_dofs")
            .and_then(Value::as_u64)
            .unwrap_or(120_000) as usize,
        latency_ms,
        via: via.to_string(),
    })
}

pub fn local_lan_ipv4s() -> Vec<Ipv4Addr> {
    let mut ips = Vec::new();
    for target in ["192.168.0.1:80", "10.0.0.1:80", "8.8.8.8:80", "100.100.100.100:80"] {
        if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
            if sock.connect(target).is_ok() {
                if let Ok(SocketAddr::V4(v4)) = sock.local_addr() {
                    let ip = *v4.ip();
                    if !ip.is_loopback() && !ip.is_unspecified() && !ips.contains(&ip) {
                        ips.push(ip);
                    }
                }
            }
        }
    }
    ips
}

pub fn discover_peers(
    ports: &[u16],
    timeout_ms: u64,
    extra_hosts: &[String],
    include_subnet_sweep: bool,
) -> Vec<DiscoveredPeer> {
    let ports = if ports.is_empty() {
        vec![5173, 8080, 18789, 18791]
    } else {
        ports.to_vec()
    };
    let timeout_ms = timeout_ms.clamp(50, 2000);
    let mut found: BTreeMap<String, DiscoveredPeer> = BTreeMap::new();

    // 1. Fast UDP broadcast beacon on port 18790
    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
        let _ = sock.set_broadcast(true);
        let _ = sock.set_read_timeout(Some(Duration::from_millis(timeout_ms.min(220))));
        let mut bcast_targets = vec![SocketAddrV4::new(Ipv4Addr::BROADCAST, DISCOVERY_UDP_PORT)];
        for ip in local_lan_ipv4s() {
            let oct = ip.octets();
            let sub_bcast = Ipv4Addr::new(oct[0], oct[1], oct[2], 255);
            let target = SocketAddrV4::new(sub_bcast, DISCOVERY_UDP_PORT);
            if !bcast_targets.contains(&target) {
                bcast_targets.push(target);
            }
        }
        let t0 = Instant::now();
        for target in bcast_targets {
            let _ = sock.send_to(DISCOVERY_MAGIC, target);
        }
        let mut buf = [0u8; 2048];
        while t0.elapsed() < Duration::from_millis(timeout_ms.min(220)) {
            match sock.recv_from(&mut buf) {
                Ok((n, SocketAddr::V4(src))) => {
                    if let Ok(val) = serde_json::from_slice::<Value>(&buf[..n]) {
                        if val.get("server").and_then(Value::as_str) == Some("fieldsmith-compute") {
                            let port = val
                                .get("listen_port")
                                .and_then(Value::as_u64)
                                .and_then(|p| u16::try_from(p).ok())
                                .filter(|&p| p > 0)
                                .unwrap_or(8080);
                            let host = src.ip().to_string();
                            let url = format!("http://{host}:{port}");
                            let peer = DiscoveredPeer {
                                url: url.clone(),
                                host: host.clone(),
                                port,
                                hostname: val
                                    .get("hostname")
                                    .and_then(Value::as_str)
                                    .unwrap_or(&host)
                                    .to_string(),
                                os: val
                                    .get("os")
                                    .and_then(Value::as_str)
                                    .unwrap_or("unknown")
                                    .to_string(),
                                arch: val
                                    .get("arch")
                                    .and_then(Value::as_str)
                                    .unwrap_or("unknown")
                                    .to_string(),
                                threads: val
                                    .get("threads")
                                    .and_then(Value::as_u64)
                                    .unwrap_or(1) as usize,
                                version: val
                                    .get("version")
                                    .and_then(Value::as_str)
                                    .unwrap_or("0.0.0")
                                    .to_string(),
                                max_recommended_dofs: val
                                    .get("max_recommended_dofs")
                                    .and_then(Value::as_u64)
                                    .unwrap_or(120_000) as usize,
                                latency_ms: t0.elapsed().as_millis() as u64,
                                via: "udp-beacon".to_string(),
                            };
                            found.insert(url, peer);
                        }
                    }
                }
                _ => break,
            }
        }
    }

    // 2. Explicit extra_hosts + loopback + /24 subnet sweep
    let mut candidates: Vec<(String, u16, &'static str)> = Vec::new();
    for h in extra_hosts {
        let h_trim = h.trim();
        if h_trim.is_empty() {
            continue;
        }
        if let Ok((host, port, _)) = parse_http_host_port(h_trim) {
            if h_trim.contains(':') {
                candidates.push((host, port, "explicit"));
            } else {
                for &p in &ports {
                    candidates.push((host.clone(), p, "explicit"));
                }
            }
        }
    }
    for &p in &ports {
        candidates.push(("127.0.0.1".to_string(), p, "loopback"));
    }
    if include_subnet_sweep {
        for ip in local_lan_ipv4s() {
            let oct = ip.octets();
            if oct[0] == 192 || oct[0] == 10 || (oct[0] == 172 && (16..=31).contains(&oct[1])) {
                for last in 1u8..=254u8 {
                    let host = format!("{}.{}.{}.{}", oct[0], oct[1], oct[2], last);
                    for &p in &ports {
                        candidates.push((host.clone(), p, "subnet-scan"));
                    }
                }
            }
        }
    }

    // Deduplicate and probe in bounded worker chunks (128 concurrent TCP connect probes)
    let mut unique: Vec<(String, u16, &'static str)> = Vec::new();
    for item in candidates {
        let url = format!("http://{}:{}", item.0, item.1);
        if !found.contains_key(&url) && !unique.iter().any(|u| u.0 == item.0 && u.1 == item.1) {
            unique.push(item);
        }
    }

    let tcp_probe_timeout_ms = timeout_ms.min(90);
    for chunk in unique.chunks(128) {
        let mut handles = Vec::with_capacity(chunk.len());
        for (host, port, via) in chunk.iter().cloned() {
            handles.push(std::thread::spawn(move || {
                probe_peer_health(&host, port, tcp_probe_timeout_ms, via)
            }));
        }
        for h in handles {
            if let Ok(Some(peer)) = h.join() {
                found.insert(peer.url.clone(), peer);
            }
        }
    }

    let mut peers: Vec<DiscoveredPeer> = found.into_values().collect();
    peers.sort_by(|a, b| {
        b.threads
            .cmp(&a.threads)
            .then_with(|| a.latency_ms.cmp(&b.latency_ms))
    });
    peers
}

fn handle_connection(
    mut stream: TcpStream,
    cfg: &ServeConfig,
    mcp_lock: &Mutex<()>,
    listen_port: u16,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return write_json(&mut stream, 400, &json!({ "error": "bad request line" }));
    }
    let method = parts[0];
    let raw_target = parts[1];
    let path_only = raw_target.split('?').next().unwrap_or("/");

    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                content_length = v.trim().parse::<usize>().unwrap_or(0);
            }
        }
    }

    if method == "OPTIONS" {
        return write_empty_cors(&mut stream, 204);
    }

    let mut body_bytes = vec![0u8; content_length.min(64 * 1024 * 1024)];
    if content_length > 0 {
        reader.read_exact(&mut body_bytes)?;
    }
    let body_str = String::from_utf8_lossy(&body_bytes);

    match (method, path_only) {
        ("GET", "/api/health") => {
            let ws = read_workspace_file(&cfg.workspace_path);
            let payload = build_health_payload(&ws, Some(listen_port));
            write_json(&mut stream, 200, &payload)
        }
        ("GET", "/api/peers/discover") | ("POST", "/api/peers/discover") => {
            let incoming: Value = if body_str.trim().is_empty() {
                json!({})
            } else {
                serde_json::from_str(&body_str).unwrap_or_else(|_| json!({}))
            };
            let ports: Vec<u16> = incoming
                .get("ports")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_u64)
                        .filter_map(|p| u16::try_from(p).ok())
                        .collect()
                })
                .filter(|v: &Vec<u16>| !v.is_empty())
                .unwrap_or_else(|| vec![8080, 18789]);
            let timeout_ms = incoming
                .get("timeout_ms")
                .and_then(Value::as_u64)
                .unwrap_or(180);
            let extra_hosts: Vec<String> = incoming
                .get("extra_hosts")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            let include_subnet = incoming
                .get("include_subnet_sweep")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let peers = discover_peers(&ports, timeout_ms, &extra_hosts, include_subnet);
            let ws = read_workspace_file(&cfg.workspace_path);
            write_json(
                &mut stream,
                200,
                &json!({
                    "peers": peers,
                    "active_remote_compute_url": ws.remote_compute_url,
                    "local_hostname": local_hostname(),
                    "local_threads": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
                }),
            )
        }
        ("POST", "/api/peers/connect") => {
            let _guard = mcp_lock.lock().unwrap_or_else(|e| e.into_inner());
            let incoming: Value = match serde_json::from_str(&body_str) {
                Ok(v) => v,
                Err(e) => return write_json(&mut stream, 400, &json!({ "error": e.to_string() })),
            };
            let url_arg = incoming
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let mut ws = read_workspace_file(&cfg.workspace_path);
            if url_arg.is_empty() || url_arg == "none" || url_arg == "disconnect" {
                ws.remote_compute_url = None;
                ws.revision += 1;
                ws.updated_at_ms = now_ms();
                ws.actor = "peer-bridge".to_string();
                ws.last_tool = "disconnect_compute_peer".to_string();
                write_workspace_file(&cfg.workspace_path, &ws);
                return write_json(
                    &mut stream,
                    200,
                    &json!({
                        "status": "disconnected",
                        "remote_compute_url": null,
                        "workspace_revision": ws.revision,
                    }),
                );
            }
            let target_url = if url_arg == "auto" {
                let peers = discover_peers(&[8080, 18789], 180, &[], true);
                let best = peers
                    .into_iter()
                    .find(|p| p.port != listen_port || (p.host != "127.0.0.1" && p.host != "localhost"))
                    .ok_or_else(|| "no external Fieldsmith compute peers discovered on subnet".to_string());
                match best {
                    Ok(p) => p.url,
                    Err(msg) => return write_json(&mut stream, 404, &json!({ "error": msg })),
                }
            } else {
                match parse_http_host_port(&url_arg) {
                    Ok((_, _, norm)) => norm,
                    Err(msg) => return write_json(&mut stream, 400, &json!({ "error": msg })),
                }
            };
            let (host, port, norm_url) = match parse_http_host_port(&target_url) {
                Ok(t) => t,
                Err(msg) => return write_json(&mut stream, 400, &json!({ "error": msg })),
            };
            let Some(peer_info) = probe_peer_health(&host, port, 1500, "connect") else {
                return write_json(
                    &mut stream,
                    502,
                    &json!({ "error": format!("failed to reach Fieldsmith compute server at {norm_url}/api/health") }),
                );
            };
            // Push current workspace (.fsm + coupled system parameters) to the remote peer
            let sync_payload = json!({
                "actor": local_hostname(),
                "last_tool": "connect_compute_peer",
                "summary": format!("Synced workspace from {} to compute peer {}", local_hostname(), peer_info.hostname),
                "doc_fsm": ws.doc_fsm,
                "coupled_system_id": ws.coupled_system_id,
                "design_p": ws.design_p,
                "epistemic_theta": ws.epistemic_theta,
            });
            let _ = http_request_json(
                &norm_url,
                "POST",
                "/api/workspace",
                Some(&sync_payload.to_string()),
                2000,
            );
            ws.remote_compute_url = Some(norm_url.clone());
            ws.revision += 1;
            ws.updated_at_ms = now_ms();
            ws.actor = "peer-bridge".to_string();
            ws.last_tool = "connect_compute_peer".to_string();
            ws.activity_log.push(json!({
                "revision": ws.revision,
                "timestamp_ms": ws.updated_at_ms,
                "actor": "peer-bridge",
                "tool": "connect_compute_peer",
                "summary": format!("Connected to remote compute peer {} ({}, {} threads)", norm_url, peer_info.hostname, peer_info.threads),
            }));
            write_workspace_file(&cfg.workspace_path, &ws);
            write_json(
                &mut stream,
                200,
                &json!({
                    "status": "connected",
                    "remote_compute_url": norm_url,
                    "peer": peer_info,
                    "workspace_revision": ws.revision,
                }),
            )
        }
        ("GET", "/api/workspace") => {
            let ws = read_workspace_file(&cfg.workspace_path);
            let val = serde_json::to_value(ws).unwrap_or_else(|_| json!({}));
            write_json(&mut stream, 200, &val)
        }
        ("POST", "/api/workspace") => {
            let _guard = mcp_lock.lock().unwrap_or_else(|e| e.into_inner());
            let incoming: Value = match serde_json::from_str(&body_str) {
                Ok(v) => v,
                Err(e) => return write_json(&mut stream, 400, &json!({ "error": e.to_string() })),
            };
            let mut ws = read_workspace_file(&cfg.workspace_path);
            ws.revision += 1;
            ws.updated_at_ms = now_ms();
            ws.actor = incoming
                .get("actor")
                .and_then(Value::as_str)
                .unwrap_or("web-ui")
                .to_string();
            ws.last_tool = incoming
                .get("last_tool")
                .and_then(Value::as_str)
                .unwrap_or("ui-sync")
                .to_string();
            if let Some(doc_fsm) = incoming.get("doc_fsm").and_then(Value::as_str) {
                ws.doc_fsm = Some(doc_fsm.to_string());
            }
            if let Some(cid) = incoming.get("coupled_system_id").and_then(Value::as_str) {
                ws.coupled_system_id = Some(cid.to_string());
            }
            if let Some(arr) = incoming.get("design_p").and_then(Value::as_array) {
                ws.design_p = Some(arr.iter().filter_map(Value::as_f64).collect());
            }
            if let Some(arr) = incoming.get("epistemic_theta").and_then(Value::as_array) {
                ws.epistemic_theta = Some(arr.iter().filter_map(Value::as_f64).collect());
            }
            if let Some(rurl) = incoming.get("remote_compute_url") {
                if rurl.is_null() {
                    ws.remote_compute_url = None;
                } else if let Some(s) = rurl.as_str() {
                    let t = s.trim();
                    ws.remote_compute_url = if t.is_empty() { None } else { Some(t.to_string()) };
                }
            }
            let summary = incoming
                .get("summary")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("Updated from {}", ws.actor));
            ws.activity_log.push(json!({
                "revision": ws.revision,
                "timestamp_ms": ws.updated_at_ms,
                "actor": ws.actor,
                "tool": ws.last_tool,
                "summary": summary,
            }));
            if ws.activity_log.len() > 25 {
                let drain = ws.activity_log.len() - 25;
                ws.activity_log.drain(0..drain);
            }
            write_workspace_file(&cfg.workspace_path, &ws);
            let val = serde_json::to_value(ws).unwrap_or_else(|_| json!({}));
            write_json(&mut stream, 200, &val)
        }
        ("POST", "/api/mcp") => {
            let _guard = mcp_lock.lock().unwrap_or_else(|e| e.into_inner());
            let parsed: Value = match serde_json::from_str(&body_str) {
                Ok(v) => v,
                Err(e) => return write_json(&mut stream, 400, &json!({ "error": e.to_string() })),
            };
            let rpc_req = if parsed.get("jsonrpc").and_then(Value::as_str) == Some("2.0") {
                parsed
            } else {
                let id = parsed.get("id").cloned().unwrap_or_else(|| json!(1));
                let method = parsed
                    .get("method")
                    .and_then(Value::as_str)
                    .unwrap_or("tools/call");
                let params = parsed.get("params").cloned().unwrap_or_else(|| {
                    let tool_name = parsed
                        .get("tool")
                        .or_else(|| parsed.get("name"))
                        .cloned()
                        .unwrap_or(Value::Null);
                    let args = parsed
                        .get("arguments")
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    json!({
                        "name": tool_name,
                        "arguments": args,
                    })
                });
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": method,
                    "params": params,
                })
            };

            let mut srv = Server::with_workspace(&cfg.workspace_path);
            let reply_str = srv
                .handle(&rpc_req.to_string())
                .unwrap_or_else(|| "{}".to_string());
            let rpc_val: Value =
                serde_json::from_str(&reply_str).unwrap_or_else(|_| json!({ "raw": reply_str }));
            let ws = read_workspace_file(&cfg.workspace_path);
            write_json(
                &mut stream,
                200,
                &json!({
                    "rpc": rpc_val,
                    "workspace": ws,
                }),
            )
        }
        ("POST", "/mcp") => {
            let _guard = mcp_lock.lock().unwrap_or_else(|e| e.into_inner());
            let mut srv = Server::with_workspace(&cfg.workspace_path);
            let reply_str = srv.handle(&body_str).unwrap_or_default();
            write_bytes_with_isolation(
                &mut stream,
                200,
                "application/json; charset=utf-8",
                reply_str.as_bytes(),
            )
        }
        ("GET", "/mcp") => write_bytes_with_isolation(
            &mut stream,
            405,
            "text/plain; charset=utf-8",
            b"Method Not Allowed",
        ),
        ("POST", "/api/solve") => {
            let ws = read_workspace_file(&cfg.workspace_path);
            if let Some(ref rurl) = ws.remote_compute_url {
                if let Ok((code, val, _)) =
                    http_request_json(rurl, "POST", "/api/solve", Some(&body_str), 60_000)
                {
                    return write_json(&mut stream, code, &val);
                }
            }
            let answer = respond(&body_str);
            write_bytes_with_isolation(
                &mut stream,
                if answer.is_error { 400 } else { 200 },
                "application/json; charset=utf-8",
                answer.body.as_bytes(),
            )
        }
        ("GET", p) => {
            if let Some(ref web_dir) = cfg.web_dir {
                serve_static_file(&mut stream, web_dir, p)
            } else {
                write_json(
                    &mut stream,
                    404,
                    &json!({ "error": "not found (no --web-dir configured)" }),
                )
            }
        }
        _ => write_json(&mut stream, 404, &json!({ "error": "not found" })),
    }
}

fn serve_static_file(stream: &mut TcpStream, web_dir: &Path, req_path: &str) -> std::io::Result<()> {
    let rel = req_path.trim_start_matches('/');
    let rel_clean = if rel.is_empty() { "index.html" } else { rel };
    let mut candidate = web_dir.to_path_buf();
    for comp in Path::new(rel_clean).components() {
        if let Component::Normal(seg) = comp {
            candidate.push(seg);
        }
    }
    if candidate.is_dir() {
        candidate.push("index.html");
    }
    if !candidate.exists() {
        let fallback = web_dir.join("index.html");
        if fallback.exists() && !rel_clean.contains('.') {
            candidate = fallback;
        } else {
            return write_json(stream, 404, &json!({ "error": "static asset not found" }));
        }
    }
    let bytes = std::fs::read(&candidate)?;
    let mime = mime_for_path(&candidate);
    write_bytes_with_isolation(stream, 200, mime, &bytes)
}

fn mime_for_path(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "wasm" => "application/wasm",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
}

fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Internal Server Error",
    }
}

fn write_empty_cors(stream: &mut TcpStream, code: u16) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {code} {}\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type, Authorization, Mcp-Session-Id, Mcp-Protocol-Version\r\n\
         Content-Length: 0\r\n\
         Connection: close\r\n\r\n",
        status_text(code)
    );
    stream.write_all(head.as_bytes())?;
    stream.flush()
}

fn write_json(stream: &mut TcpStream, code: u16, body: &Value) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(body).unwrap_or_else(|_| b"{}".to_vec());
    write_bytes_with_isolation(stream, code, "application/json; charset=utf-8", &bytes)
}

fn write_bytes_with_isolation(
    stream: &mut TcpStream,
    code: u16,
    content_type: &str,
    bytes: &[u8],
) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {code} {}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Cross-Origin-Opener-Policy: same-origin\r\n\
         Cross-Origin-Embedder-Policy: require-corp\r\n\
         Cross-Origin-Resource-Policy: cross-origin\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type, Authorization, Mcp-Session-Id, Mcp-Protocol-Version\r\n\
         Connection: close\r\n\r\n",
        status_text(code),
        bytes.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(bytes)?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_server_health_and_mcp_roundtrip() {
        let tmp_ws = std::env::temp_dir().join(format!(
            "fieldsmith-http-test-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&tmp_ws);

        let srv = spawn_http_server(ServeConfig {
            bind_addr: "127.0.0.1:0".to_string(),
            workspace_path: tmp_ws.clone(),
            web_dir: None,
        })
        .expect("spawn http server");
        let addr = srv.local_addr();

        // 1. GET /api/health
        let mut s1 = TcpStream::connect(addr).expect("connect health");
        s1.write_all(b"GET /api/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .expect("write health");
        let mut resp1 = String::new();
        s1.read_to_string(&mut resp1).expect("read health");
        assert!(resp1.contains("200 OK"), "resp1={resp1}");
        assert!(resp1.contains("fieldsmith-compute"), "resp1={resp1}");

        // 2. POST /api/mcp (new_document)
        let body = r#"{"tool":"new_document","arguments":{"name":"mobile_test","material":"pla"}}"#;
        let req2 = format!(
            "POST /api/mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let mut s2 = TcpStream::connect(addr).expect("connect mcp");
        s2.write_all(req2.as_bytes()).expect("write mcp");
        let mut resp2 = String::new();
        s2.read_to_string(&mut resp2).expect("read mcp");
        assert!(resp2.contains("200 OK"), "resp2={resp2}");
        assert!(resp2.contains("mobile_test"), "resp2={resp2}");

        // 3. POST /mcp (Streamable HTTP JSON-RPC 2.0)
        let rpc_init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0.1.0"}}}"#;
        let req3 = format!(
            "POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            rpc_init.len(),
            rpc_init
        );
        let mut s3 = TcpStream::connect(addr).expect("connect /mcp");
        s3.write_all(req3.as_bytes()).expect("write /mcp");
        let mut resp3 = String::new();
        s3.read_to_string(&mut resp3).expect("read /mcp");
        assert!(resp3.contains("200 OK"), "resp3={resp3}");
        assert!(resp3.contains("fieldsmith-mcp"), "resp3={resp3}");

        let _ = std::fs::remove_file(&tmp_ws);
    }

    #[test]
    fn peer_discovery_and_compute_offload_bridge() {
        let pid = std::process::id();
        let ws_phone = std::env::temp_dir().join(format!("fieldsmith-phone-ws-{pid}.json"));
        let ws_workstation =
            std::env::temp_dir().join(format!("fieldsmith-workstation-ws-{pid}.json"));
        let _ = std::fs::remove_file(&ws_phone);
        let _ = std::fs::remove_file(&ws_workstation);

        let srv_phone = spawn_http_server(ServeConfig {
            bind_addr: "127.0.0.1:0".to_string(),
            workspace_path: ws_phone.clone(),
            web_dir: None,
        })
        .expect("spawn phone server");
        let srv_workstation = spawn_http_server(ServeConfig {
            bind_addr: "127.0.0.1:0".to_string(),
            workspace_path: ws_workstation.clone(),
            web_dir: None,
        })
        .expect("spawn workstation server");

        let phone_url = format!("http://127.0.0.1:{}", srv_phone.local_addr().port());
        let ws_port = srv_workstation.local_addr().port();
        let workstation_url = format!("http://127.0.0.1:{ws_port}");

        // 1. Discover workstation from phone via /api/peers/discover (without full /24 sweep for fast unit test)
        let disc_body = json!({
            "ports": [ws_port],
            "timeout_ms": 200,
            "include_subnet_sweep": false,
        })
        .to_string();
        let (code, disc_val, _) =
            http_request_json(&phone_url, "POST", "/api/peers/discover", Some(&disc_body), 2000)
                .expect("discover request");
        assert_eq!(code, 200, "disc_val={disc_val}");
        let peers = disc_val
            .get("peers")
            .and_then(Value::as_array)
            .expect("peers array");
        assert!(
            peers.iter().any(|p| p.get("port").and_then(Value::as_u64) == Some(u64::from(ws_port))),
            "expected workstation port {ws_port} in discovered peers: {peers:?}"
        );

        // 2. Connect phone to workstation via /api/peers/connect
        let conn_body = json!({ "url": workstation_url }).to_string();
        let (code2, conn_val, _) =
            http_request_json(&phone_url, "POST", "/api/peers/connect", Some(&conn_body), 2000)
                .expect("connect request");
        assert_eq!(code2, 200, "conn_val={conn_val}");
        assert_eq!(
            conn_val.get("status").and_then(Value::as_str),
            Some("connected")
        );

        // 3. Create a simple cantilever document on phone and run `solve` via MCP; verify `offloaded_to`
        for step in [
            json!({"tool": "new_document", "arguments": {"name": "peer_beam", "material": "pla"}}),
            json!({"tool": "upsert_shape", "arguments": {"id": "b", "kind": "box", "size": [20.0, 10.0, 10.0]}}),
            json!({"tool": "add_region", "arguments": {"id": "r1", "selectors": [{"shape": "b", "face": "nx"}]}}),
            json!({"tool": "add_region", "arguments": {"id": "r2", "selectors": [{"shape": "b", "face": "px"}]}}),
            json!({"tool": "add_support", "arguments": {"id": "s1", "region": "r1", "fixed": [true, true, true]}}),
            json!({"tool": "add_load", "arguments": {"id": "l1", "region": "r2", "force": [0.0, 0.0, -50.0]}}),
        ] {
            let (code, val, _) =
                http_request_json(&phone_url, "POST", "/api/mcp", Some(&step.to_string()), 2000)
                    .expect("setup step");
            assert_eq!(code, 200, "step={step} val={val}");
        }

        let solve_body = json!({
            "tool": "solve",
            "arguments": { "kind": "solve", "dofs": 1500 }
        })
        .to_string();
        let (code4, solve_val, _) =
            http_request_json(&phone_url, "POST", "/api/mcp", Some(&solve_body), 15_000)
                .expect("solve via mcp");
        assert_eq!(code4, 200, "solve_val={solve_val}");
        let text = solve_val
            .pointer("/rpc/result/content/0/text")
            .and_then(Value::as_str)
            .expect("solve text");
        let parsed_solve: Value =
            serde_json::from_str(text).unwrap_or_else(|e| panic!("parse solve json ({e}): text={text}"));
        assert_eq!(
            parsed_solve.get("offloaded_to").and_then(Value::as_str),
            Some(workstation_url.as_str()),
            "expected offloaded_to == {workstation_url}, got {parsed_solve}"
        );

        let _ = std::fs::remove_file(&ws_phone);
        let _ = std::fs::remove_file(&ws_workstation);
    }
}
