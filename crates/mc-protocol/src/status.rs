//! Server list ping: the status handshake of `OldServerPinger.ping` (`C00Handshake` with next
//! state 1, `C00PacketServerQuery`, `S00PacketServerInfo`, `C01PacketPing`, `S01PacketPong`)
//! and its legacy `0xFE` fallback (`tryCompatibilityPing`).
//!
//! The status state is unencrypted and uncompressed, so this talks to the socket directly with
//! [`frame`] rather than the play-state `net::Reader`. Vanilla's pipeline has a 30 s
//! `ReadTimeoutHandler` and a 30 s connect timeout; both are polled in short slices so a
//! cancelled ping (screen closed) stops promptly.

use crate::frame::{self, Decoder};
use crate::packets::*;
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
pub const READ_TIMEOUT: Duration = Duration::from_secs(30);
const SLICE: Duration = Duration::from_millis(200);

/// `ServerAddress`: host and port (the SRV lookup of `getServerAddress` is not done).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Address {
    pub host: String,
    pub port: u16,
}

/// `ServerAddress.fromString`
pub fn parse_address(s: &str) -> Address {
    let int = |t: &str| t.trim().parse::<i32>().ok().filter(|p| (0..=65535).contains(p)).map(|p| p as u16);
    let mut parts: Vec<&str> = s.split(':').collect();
    // Java's split drops trailing empty strings.
    while parts.len() > 1 && parts.last() == Some(&"") {
        parts.pop();
    }
    let mut bracket: Option<(String, Option<String>)> = None;
    if s.starts_with('[') {
        if let Some(i) = s.find(']').filter(|&i| i > 0) {
            let host = s[1..i].to_string();
            let rest = s[i + 1..].trim();
            bracket = Some((host, rest.strip_prefix(':').filter(|r| !r.is_empty()).map(str::to_string)));
        }
    }
    if let Some((host, port)) = bracket {
        return Address { host, port: port.as_deref().and_then(int).unwrap_or(25565) };
    }
    if parts.len() > 2 {
        return Address { host: s.to_string(), port: 25565 };
    }
    Address { host: parts[0].to_string(), port: parts.get(1).and_then(|p| int(p)).unwrap_or(25565) }
}

#[derive(Debug)]
pub enum PingError {
    /// `UnknownHostException`
    UnknownHost,
    Io(io::Error),
    Protocol(String),
    Cancelled,
}

impl From<io::Error> for PingError {
    fn from(e: io::Error) -> Self {
        PingError::Io(e)
    }
}

fn connect(addr: &Address, cancel: &AtomicBool, timeout: Duration) -> Result<TcpStream, PingError> {
    let addrs: Vec<_> = (addr.host.as_str(), addr.port).to_socket_addrs().map_err(|_| PingError::UnknownHost)?.collect();
    if addrs.is_empty() {
        return Err(PingError::UnknownHost);
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(PingError::Cancelled);
    }
    let mut last = None;
    for a in addrs {
        match TcpStream::connect_timeout(&a, timeout) {
            Ok(s) => {
                s.set_nodelay(true)?;
                s.set_read_timeout(Some(SLICE))?;
                return Ok(s);
            }
            Err(e) => last = Some(e),
        }
    }
    Err(PingError::Io(last.unwrap()))
}

/// Waits for the next packet body, polling `cancel` and giving up after `timeout`.
fn recv(stream: &mut TcpStream, dec: &mut Decoder, cancel: &AtomicBool, timeout: Duration) -> Result<Vec<u8>, PingError> {
    let start = Instant::now();
    let mut buf = [0u8; 8192];
    loop {
        if let Some(p) = dec.next_packet().map_err(|e| PingError::Protocol(format!("{e:?}")))? {
            return Ok(p);
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(PingError::Cancelled);
        }
        if start.elapsed() > timeout {
            return Err(PingError::Io(io::Error::new(io::ErrorKind::TimedOut, "read timed out")));
        }
        match stream.read(&mut buf) {
            Ok(0) => return Err(PingError::Io(io::Error::new(io::ErrorKind::UnexpectedEof, "End of stream"))),
            Ok(n) => dec.feed(&buf[..n]),
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {}
            Err(e) => return Err(e.into()),
        }
    }
}

fn send(stream: &mut TcpStream, body: &[u8]) -> io::Result<()> {
    let mut out = Vec::with_capacity(body.len() + 5);
    frame::encode(body, -1, &mut out);
    stream.write_all(&out)
}

/// The modern status ping. `on_status` gets the `S00PacketServerInfo` JSON as soon as it
/// arrives (vanilla fills `ServerData` from it before the pong); the result is the round trip
/// of `C01PacketPing` -> `S01PacketPong` in ms.
pub fn ping(addr: &Address, cancel: &AtomicBool, on_status: &mut dyn FnMut(&str)) -> Result<u64, PingError> {
    let mut stream = connect(addr, cancel, CONNECT_TIMEOUT)?;
    let mut dec = Decoder::new();
    send(&mut stream, &HandshakeServerbound::Handshake(Handshake { protocol_version: crate::PROTOCOL_VERSION, address: addr.host.clone(), port: addr.port, next_state: 1 }).to_bytes())?;
    send(&mut stream, &StatusServerbound::StatusRequest(StatusRequest {}).to_bytes())?;
    let body = recv(&mut stream, &mut dec, cancel, READ_TIMEOUT)?;
    match StatusClientbound::read(&body).map_err(|e| PingError::Protocol(format!("{e:?}")))? {
        StatusClientbound::StatusResponse(s) => on_status(&s.json),
        _ => return Err(PingError::Protocol("Received unrequested packet".into())),
    }
    let sent = Instant::now();
    send(&mut stream, &StatusServerbound::StatusPing(StatusPing { client_time: 0 }).to_bytes())?;
    let body = recv(&mut stream, &mut dec, cancel, READ_TIMEOUT)?;
    match StatusClientbound::read(&body).map_err(|e| PingError::Protocol(format!("{e:?}")))? {
        StatusClientbound::StatusPong(_) => Ok(sent.elapsed().as_millis() as u64),
        _ => Err(PingError::Protocol("Received unrequested packet".into())),
    }
}

/// The reply of the pre-1.7 `0xFE 0x01` ping (`§1\0protocol\0version\0motd\0online\0max`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyStatus {
    pub game_version: String,
    pub motd: String,
    pub online: i32,
    pub max: i32,
}

/// The bytes `tryCompatibilityPing` sends: `FE 01 FA "MC|PingHost" <len> 7F host port`.
pub fn legacy_request(addr: &Address) -> Vec<u8> {
    let mut out = vec![0xFE, 0x01, 0xFA];
    let utf16 = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
    let tag = utf16("MC|PingHost");
    out.extend((tag.len() as u16).to_be_bytes());
    tag.iter().for_each(|c| out.extend(c.to_be_bytes()));
    let host = utf16(&addr.host);
    out.extend((7 + 2 * host.len() as u16).to_be_bytes());
    out.push(127);
    out.extend((host.len() as u16).to_be_bytes());
    host.iter().for_each(|c| out.extend(c.to_be_bytes()));
    out.extend((addr.port as i32).to_be_bytes());
    out
}

/// `channelRead0` of the compatibility handler; `None` unless the reply is a `§1` kick packet.
pub fn parse_legacy(data: &[u8]) -> Option<LegacyStatus> {
    if data.first() != Some(&0xFF) || data.len() < 3 {
        return None;
    }
    let n = u16::from_be_bytes([data[1], data[2]]) as usize * 2;
    let text = data.get(3..3 + n)?;
    let units: Vec<u16> = text.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
    let s = String::from_utf16_lossy(&units);
    // Splitter.on('\0').limit(6)
    let parts: Vec<&str> = s.splitn(6, '\0').collect();
    if parts.len() < 6 || parts[0] != "\u{a7}1" {
        return None;
    }
    let int = |t: &str, d: i32| t.trim().parse::<i32>().unwrap_or(d);
    Some(LegacyStatus { game_version: parts[2].to_string(), motd: parts[3].to_string(), online: int(parts[4], -1), max: int(parts[5], -1) })
}

/// `tryCompatibilityPing`
pub fn legacy_ping(addr: &Address, cancel: &AtomicBool) -> Result<Option<LegacyStatus>, PingError> {
    let mut stream = connect(addr, cancel, CONNECT_TIMEOUT)?;
    stream.write_all(&legacy_request(addr))?;
    let start = Instant::now();
    let mut data = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        if let Some(r) = parse_legacy(&data) {
            return Ok(Some(r));
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(PingError::Cancelled);
        }
        if start.elapsed() > READ_TIMEOUT {
            return Ok(None);
        }
        match stream.read(&mut buf) {
            Ok(0) => return Ok(parse_legacy(&data)),
            Ok(n) => data.extend_from_slice(&buf[..n]),
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {}
            Err(e) => return Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn addresses_parse_like_server_address() {
        let a = |h: &str, p: u16| Address { host: h.into(), port: p };
        assert_eq!(parse_address("example.com"), a("example.com", 25565));
        assert_eq!(parse_address("example.com:25566"), a("example.com", 25566));
        assert_eq!(parse_address("example.com:abc"), a("example.com", 25565));
        assert_eq!(parse_address("[::1]:1234"), a("::1", 1234));
        assert_eq!(parse_address("[::1]"), a("::1", 25565));
        assert_eq!(parse_address("::1"), a("::1", 25565));
        assert_eq!(parse_address("example.com:"), a("example.com", 25565));
    }

    #[test]
    fn legacy_request_layout() {
        let r = legacy_request(&Address { host: "ab".into(), port: 25565 });
        assert_eq!(&r[..3], &[0xFE, 0x01, 0xFA]);
        assert_eq!(&r[3..5], &[0, 11]);
        // 11 UTF-16 units of "MC|PingHost", then length 7 + 2*2, protocol 127, host, port
        let tail = &r[5 + 22..];
        assert_eq!(tail, &[0, 11, 127, 0, 2, 0, b'a', 0, b'b', 0, 0, 0x63, 0xDD]);
    }

    fn kick(parts: &[&str]) -> Vec<u8> {
        let s = parts.join("\0");
        let units: Vec<u16> = s.encode_utf16().collect();
        let mut out = vec![0xFF];
        out.extend((units.len() as u16).to_be_bytes());
        units.iter().for_each(|u| out.extend(u.to_be_bytes()));
        out
    }

    #[test]
    fn legacy_reply_parses() {
        let r = parse_legacy(&kick(&["\u{a7}1", "127", "1.6.4", "Old Server", "3", "20"])).unwrap();
        assert_eq!(r, LegacyStatus { game_version: "1.6.4".into(), motd: "Old Server".into(), online: 3, max: 20 });
        assert!(parse_legacy(&kick(&["x", "127", "1.6.4", "m", "3", "20"])).is_none());
        assert!(parse_legacy(&[0xFF, 0, 5]).is_none());
        let r = parse_legacy(&kick(&["\u{a7}1", "127", "v", "m", "?", "?"])).unwrap();
        assert_eq!((r.online, r.max), (-1, -1));
    }

    /// A one-shot status server: handshake, request -> `json`, ping -> pong.
    fn status_server(json: &'static str) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut dec = Decoder::new();
            let mut buf = [0u8; 1024];
            let mut stage = 0;
            while stage < 3 {
                let n = s.read(&mut buf).unwrap();
                if n == 0 {
                    return;
                }
                dec.feed(&buf[..n]);
                while let Ok(Some(p)) = dec.next_packet() {
                    match stage {
                        0 => {
                            let HandshakeServerbound::Handshake(h) = HandshakeServerbound::read(&p).unwrap();
                            assert_eq!((h.protocol_version, h.next_state), (47, 1));
                        }
                        1 => {
                            assert!(matches!(StatusServerbound::read(&p).unwrap(), StatusServerbound::StatusRequest(_)));
                            send(&mut s, &StatusClientbound::StatusResponse(StatusResponse { json: json.into() }).to_bytes()).unwrap();
                        }
                        _ => {
                            let StatusServerbound::StatusPing(pg) = StatusServerbound::read(&p).unwrap() else { panic!() };
                            std::thread::sleep(Duration::from_millis(30));
                            send(&mut s, &StatusClientbound::StatusPong(StatusPong { client_time: pg.client_time }).to_bytes()).unwrap();
                        }
                    }
                    stage += 1;
                }
            }
        });
        port
    }

    #[test]
    fn modern_ping_returns_status_then_latency() {
        let port = status_server(r#"{"version":{"name":"1.8.9","protocol":47},"players":{"max":20,"online":1},"description":"hi"}"#);
        let cancel = AtomicBool::new(false);
        let mut got = String::new();
        let ms = ping(&Address { host: "127.0.0.1".into(), port }, &cancel, &mut |j| got = j.to_string()).unwrap();
        assert!(got.contains("\"protocol\":47"));
        assert!((25..2000).contains(&ms), "{ms} ms");
    }

    #[test]
    fn refused_connection_is_an_io_error_and_cancel_wins() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        let a = Address { host: "127.0.0.1".into(), port };
        assert!(matches!(ping(&a, &AtomicBool::new(false), &mut |_| {}), Err(PingError::Io(_))));
        assert!(matches!(ping(&a, &AtomicBool::new(true), &mut |_| {}), Err(PingError::Cancelled)));
        let bad = Address { host: "no-such-host.invalid".into(), port: 1 };
        assert!(matches!(ping(&bad, &AtomicBool::new(false), &mut |_| {}), Err(PingError::UnknownHost)));
    }
}
