//! Client side of `NetworkManager`: a TCP stream with the vanilla pipeline (AES/CFB8 →
//! VarInt framing → zlib compression), the handshake/login sequence of `GuiConnecting` and
//! `NetHandlerLoginClient`, then independent reader and writer halves for the play state.
use crate::crypto::{random_bytes, rsa_encrypt, server_hash, Cfb8};
use crate::frame::{self, Decoder};
use crate::packets::*;
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn proto(e: crate::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("{e}"))
}

/// Outgoing half: frames, compresses and encrypts packet bodies.
pub struct Writer {
    stream: TcpStream,
    cipher: Option<Cfb8>,
    threshold: Arc<AtomicI32>,
}

impl Writer {
    /// Sends one packet body (id + fields) and flushes, like `NetworkManager.sendPacket`.
    pub fn send(&mut self, body: &[u8]) -> io::Result<()> {
        let mut out = Vec::with_capacity(body.len() + 5);
        frame::encode(body, self.threshold.load(Ordering::Relaxed), &mut out);
        if let Some(c) = &mut self.cipher {
            c.encrypt(&mut out);
        }
        self.stream.write_all(&out)
    }
    pub fn send_play(&mut self, p: &PlayServerbound) -> io::Result<()> {
        self.send(&p.to_bytes())
    }
    pub fn shutdown(&self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

/// Incoming half: decrypts, deframes and inflates packet bodies.
pub struct Reader {
    stream: TcpStream,
    cipher: Option<Cfb8>,
    decoder: Decoder,
    threshold: Arc<AtomicI32>,
    buf: Vec<u8>,
}

impl Reader {
    /// Blocks for the next packet body.
    pub fn recv(&mut self) -> io::Result<Vec<u8>> {
        loop {
            self.decoder.threshold = self.threshold.load(Ordering::Relaxed);
            if let Some(p) = self.decoder.next_packet().map_err(proto)? {
                return Ok(p);
            }
            let n = self.stream.read(&mut self.buf).map_err(|e| match e.kind() {
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => io::Error::new(io::ErrorKind::TimedOut, "Timed out"),
                _ => e,
            });
            let n = match n {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                n => n?,
            };
            if n == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "End of stream"));
            }
            let data = &mut self.buf[..n];
            if let Some(c) = &mut self.cipher {
                c.decrypt(data);
            }
            self.decoder.feed(data);
        }
    }
    pub fn recv_play(&mut self) -> io::Result<PlayClientbound> {
        let body = self.recv()?;
        let p = PlayClientbound::read(&body).map_err(proto)?;
        if let PlayClientbound::SetCompression(c) = &p {
            // S46PacketSetCompressionLevel takes effect for everything after it.
            self.set_threshold(c.threshold);
        }
        Ok(p)
    }
    fn set_threshold(&mut self, t: i32) {
        self.threshold.store(t, Ordering::Relaxed);
        self.decoder.threshold = t;
    }
}

/// Joins the session server for online-mode servers: `(access token, profile id, server hash)`.
pub type Authenticate<'a> = &'a mut dyn FnMut(&str) -> Result<(), String>;

#[derive(Debug)]
pub enum LoginError {
    Io(io::Error),
    /// `LoginDisconnect` reason (chat JSON).
    Kicked(String),
    /// Online mode needs a session; the callback's error.
    Auth(String),
    Protocol(String),
}

impl From<io::Error> for LoginError {
    fn from(e: io::Error) -> Self {
        LoginError::Io(e)
    }
}

/// `GuiConnecting.connect`: TCP connect, handshake, login start, then the login packets until
/// `LoginSuccess`. `auth` is called with the server hash when the server asks for encryption.
pub fn login(host: &str, port: u16, name: &str, auth: Option<Authenticate>) -> Result<(Reader, Writer, LoginSuccess), LoginError> {
    login_with_socket(host, port, name, auth, |_| Ok(()))
}

/// Like `login`, notifying the caller when the socket is available for cancellation.
pub fn login_with_socket(host: &str, port: u16, name: &str, auth: Option<Authenticate>, on_socket: impl FnOnce(&TcpStream) -> io::Result<()>) -> Result<(Reader, Writer, LoginSuccess), LoginError> {
    let addr = (host, port).to_socket_addrs()?.next().ok_or_else(|| LoginError::Protocol(format!("Unknown host '{host}'")))?;
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(30))?;
    on_socket(&stream)?;
    stream.set_nodelay(true)?;
    // `ReadTimeoutHandler(30)`: a silent server ends the connection instead of hanging it.
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let threshold = Arc::new(AtomicI32::new(-1));
    let mut w = Writer { stream: stream.try_clone()?, cipher: None, threshold: threshold.clone() };
    let mut r = Reader { stream, cipher: None, decoder: Decoder::new(), threshold, buf: vec![0; 64 * 1024] };
    let hs = HandshakeServerbound::Handshake(Handshake { protocol_version: crate::PROTOCOL_VERSION, address: host.into(), port, next_state: 2 });
    w.send(&hs.to_bytes())?;
    w.send(&LoginServerbound::LoginStart(LoginStart { name: name.into() }).to_bytes())?;
    let mut auth = auth;
    loop {
        let body = r.recv()?;
        match LoginClientbound::read(&body).map_err(|e| LoginError::Protocol(format!("{e:?}")))? {
            LoginClientbound::LoginDisconnect(d) => return Err(LoginError::Kicked(d.reason)),
            LoginClientbound::EncryptionRequest(req) => {
                // NetHandlerLoginClient.handleEncryptionRequest
                let secret: [u8; 16] = random_bytes(16).try_into().unwrap();
                let hash = server_hash(&req.server_id, &secret, &req.public_key);
                match auth.as_mut() {
                    Some(f) => f(&hash).map_err(LoginError::Auth)?,
                    None => return Err(LoginError::Auth("Not authenticated with Minecraft.net".into())),
                }
                let mut rng = random_bytes(4096).into_iter().cycle();
                let enc = |m: &[u8], rng: &mut dyn Iterator<Item = u8>| rsa_encrypt(&req.public_key, m, || rng.next().unwrap());
                let resp = EncryptionResponse {
                    shared_secret: enc(&secret, &mut rng).ok_or_else(|| LoginError::Protocol("bad server key".into()))?,
                    verify_token: enc(&req.verify_token, &mut rng).ok_or_else(|| LoginError::Protocol("bad server key".into()))?,
                };
                // The response itself goes out unencrypted; the ciphers start right after it.
                w.send(&LoginServerbound::EncryptionResponse(resp).to_bytes())?;
                w.cipher = Some(Cfb8::new(&secret));
                r.cipher = Some(Cfb8::new(&secret));
            }
            LoginClientbound::EnableCompression(c) => r.set_threshold(c.threshold),
            LoginClientbound::LoginSuccess(s) => return Ok((r, w, s)),
        }
    }
}

/// Server list ping (`ServerPinger`): status JSON and round-trip time in ms.
pub fn ping(host: &str, port: u16) -> io::Result<(String, u64)> {
    let addr = (host, port).to_socket_addrs()?.next().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "unknown host"))?;
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let threshold = Arc::new(AtomicI32::new(-1));
    let mut w = Writer { stream: stream.try_clone()?, cipher: None, threshold: threshold.clone() };
    let mut r = Reader { stream, cipher: None, decoder: Decoder::new(), threshold, buf: vec![0; 8192] };
    w.send(&HandshakeServerbound::Handshake(Handshake { protocol_version: crate::PROTOCOL_VERSION, address: host.into(), port, next_state: 1 }).to_bytes())?;
    w.send(&StatusServerbound::StatusRequest(StatusRequest {}).to_bytes())?;
    let json = match StatusClientbound::read(&r.recv()?).map_err(proto)? {
        StatusClientbound::StatusResponse(s) => s.json,
        _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "expected status")),
    };
    let t = std::time::Instant::now();
    w.send(&StatusServerbound::StatusPing(StatusPing { client_time: 0 }).to_bytes())?;
    let _ = r.recv()?;
    Ok((json, t.elapsed().as_millis() as u64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// A tiny offline-mode server: handshake, login start, compression, success, keep-alive.
    #[test]
    fn offline_login_and_play_roundtrip() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (s, _) = l.accept().unwrap();
            let th = Arc::new(AtomicI32::new(-1));
            let mut w = Writer { stream: s.try_clone().unwrap(), cipher: None, threshold: th.clone() };
            let mut r = Reader { stream: s, cipher: None, decoder: Decoder::new(), threshold: th.clone(), buf: vec![0; 4096] };
            let hs = HandshakeServerbound::read(&r.recv().unwrap()).unwrap();
            assert!(matches!(hs, HandshakeServerbound::Handshake(Handshake { next_state: 2, protocol_version: 47, .. })));
            let LoginServerbound::LoginStart(ls) = LoginServerbound::read(&r.recv().unwrap()).unwrap() else { panic!() };
            w.send(&LoginClientbound::EnableCompression(EnableCompression { threshold: 64 }).to_bytes()).unwrap();
            th.store(64, Ordering::Relaxed);
            w.send(&LoginClientbound::LoginSuccess(LoginSuccess { uuid: "00000000-0000-0000-0000-000000000001".into(), name: ls.name }).to_bytes()).unwrap();
            w.send(&PlayClientbound::KeepAliveS(KeepAliveS { key: 1234 }).to_bytes()).unwrap();
            let big = "x".repeat(300);
            w.send(&PlayClientbound::ChatS(ChatS { json: format!("{{\"text\":\"{big}\"}}"), position: 0 }).to_bytes()).unwrap();
            let PlayServerbound::KeepAliveC(k) = PlayServerbound::read(&r.recv().unwrap()).unwrap() else { panic!() };
            assert_eq!(k.key, 1234);
        });
        let (mut r, mut w, ok) = login("127.0.0.1", port, "Steve", None).unwrap();
        assert_eq!(ok.name, "Steve");
        let PlayClientbound::KeepAliveS(k) = r.recv_play().unwrap() else { panic!() };
        assert!(matches!(r.recv_play().unwrap(), PlayClientbound::ChatS(_)));
        w.send_play(&PlayServerbound::KeepAliveC(KeepAliveC { key: k.key })).unwrap();
        server.join().unwrap();
    }
}
