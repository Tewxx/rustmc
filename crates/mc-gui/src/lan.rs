//! `LanServerDetector`: listens for "Open to LAN" announcements on 224.0.2.60:4445.

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanServer {
    pub motd: String,
    /// `ip:port`
    pub address: String,
}

/// `LanServerDetector.LanServerList`
#[derive(Default)]
pub struct LanServerList {
    inner: Mutex<(Vec<LanServer>, bool)>,
}

impl LanServerList {
    pub fn was_updated(&self) -> bool {
        self.inner.lock().unwrap().1
    }

    /// `getLanServers` + `setWasNotUpdated`
    pub fn take(&self) -> Vec<LanServer> {
        let mut g = self.inner.lock().unwrap();
        g.1 = false;
        g.0.clone()
    }

    /// `func_77551_a`: one received announcement from `from`.
    pub fn add(&self, message: &str, from: &str) {
        let motd = motd_of(message);
        let Some(ad) = ad_of(message) else { return };
        let address = format!("{from}:{ad}");
        let mut g = self.inner.lock().unwrap();
        if !g.0.iter().any(|s| s.address == address) {
            g.0.push(LanServer { motd, address });
            g.1 = true;
        }
    }
}

/// `ThreadLanServerPing.getMotdFromPingResponse`
pub fn motd_of(s: &str) -> String {
    let Some(i) = s.find("[MOTD]") else { return "missing no".into() };
    match s[i + 6..].find("[/MOTD]") {
        Some(j) => s[i + 6..i + 6 + j].to_string(),
        None => "missing no".into(),
    }
}

/// `ThreadLanServerPing.getAdFromPingResponse`
pub fn ad_of(s: &str) -> Option<String> {
    let i = s.find("[/MOTD]")?;
    let after = i + 7;
    if s[after..].contains("[/MOTD]") {
        return None;
    }
    let k = after + s[after..].find("[AD]")?;
    let l = k + 4 + s[k + 4..].find("[/AD]")?;
    Some(s[k + 4..l].to_string())
}

/// `ThreadLanServerFind`: stops when dropped.
pub struct LanDetector {
    stop: Arc<AtomicBool>,
}

impl LanDetector {
    /// Starts listening; `None` when the socket or the multicast join fails.
    pub fn start(list: Arc<LanServerList>) -> Option<Self> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 4445)).ok()?;
        socket.join_multicast_v4(&Ipv4Addr::new(224, 0, 2, 60), &Ipv4Addr::UNSPECIFIED).ok()?;
        socket.set_read_timeout(Some(Duration::from_millis(500))).ok()?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::Builder::new()
            .name("LanServerDetector".into())
            .spawn(move || {
                let mut buf = [0u8; 1024];
                while !flag.load(Ordering::Relaxed) {
                    match socket.recv_from(&mut buf) {
                        Ok((n, SocketAddr::V4(from))) => list.add(&String::from_utf8_lossy(&buf[..n]), &from.ip().to_string()),
                        Ok(_) => {}
                        Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                        Err(_) => break,
                    }
                }
            })
            .ok()?;
        Some(Self { stop })
    }
}

impl Drop for LanDetector {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn announcement_parsing_matches_thread_lan_server_ping() {
        let m = "[MOTD]Steve - New World[/MOTD][AD]54321[/AD]";
        assert_eq!(motd_of(m), "Steve - New World");
        assert_eq!(ad_of(m).as_deref(), Some("54321"));
        assert_eq!(motd_of("garbage"), "missing no");
        assert_eq!(ad_of("garbage"), None);
        assert_eq!(ad_of("[MOTD]a[/MOTD][/MOTD][AD]1[/AD]"), None);
        assert_eq!(ad_of("[MOTD]a[/MOTD][AD]1"), None);
    }

    #[test]
    fn list_dedupes_by_address_and_flags_updates() {
        let l = LanServerList::default();
        assert!(!l.was_updated());
        l.add("[MOTD]w[/MOTD][AD]25565[/AD]", "192.168.1.5");
        l.add("[MOTD]w[/MOTD][AD]25565[/AD]", "192.168.1.5");
        l.add("nonsense", "192.168.1.6");
        assert!(l.was_updated());
        let v = l.take();
        assert_eq!(v, vec![LanServer { motd: "w".into(), address: "192.168.1.5:25565".into() }]);
        assert!(!l.was_updated());
    }
}
