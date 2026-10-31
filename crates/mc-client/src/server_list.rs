//! Server list pinging (`ServerListEntryNormal`'s pool + `OldServerPinger`) and the glue between
//! `servers.dat` (`ServerData.resourceMode`) and rustmc's per-server pack store.
//!
//! The screens, `ServerList` and the `servers.dat` file live in `mc_gui::{multiplayer,
//! server_data}`; this module installs the pinger they call on the first draw of each entry.

use crate::chat;
use mc_gui::lang::Lang;
use mc_gui::server_data::{Handle, ResourceMode, ServerData, ServerList};
use mc_protocol::status::{self, PingError};
use serde_json::Value;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, OnceLock};

type Job = (Handle, Lang, Arc<AtomicBool>);

/// `ServerListEntryNormal.field_148302_b`: five daemon threads named "Server Pinger #n".
static POOL: OnceLock<Mutex<Sender<Job>>> = OnceLock::new();

fn submit(h: Handle, lang: Lang, cancel: Arc<AtomicBool>) {
    let tx = POOL.get_or_init(|| {
        let (tx, rx) = channel::<Job>();
        let rx = Arc::new(Mutex::new(rx));
        for n in 0..5 {
            let rx = rx.clone();
            let _ = std::thread::Builder::new().name(format!("Server Pinger #{n}")).spawn(move || loop {
                let job = rx.lock().unwrap().recv();
                match job {
                    Ok((h, lang, cancel)) => ping_server(&h, &lang, &cancel),
                    Err(_) => break,
                }
            });
        }
        Mutex::new(tx)
    });
    let _ = tx.lock().unwrap().send((h, lang, cancel));
}

/// Makes the multiplayer screen ping through this module.
pub fn install() {
    mc_gui::multiplayer::set_pinger(submit);
}

const DARK_RED: &str = "\u{a7}4";

/// `OldServerPinger.handleServerInfo`: fills the server's MOTD, version, population, player
/// sample and favicon from the status JSON.
pub fn apply_status(h: &Handle, json: &str, lang: &Lang) {
    let Ok(v) = serde_json::from_str::<Value>(json) else { return };
    let mut d = h.lock().unwrap();
    d.motd = match v.get("description") {
        Some(desc) if !desc.is_null() => chat::parse(&desc.to_string(), lang).formatted(),
        _ => String::new(),
    };
    match v.get("version") {
        Some(ver) if ver.is_object() => {
            d.game_version = ver.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            d.version = ver.get("protocol").and_then(Value::as_i64).unwrap_or(0) as i32;
        }
        _ => {
            d.game_version = "Old".into();
            d.version = 0;
        }
    }
    match v.get("players") {
        Some(p) if p.is_object() => {
            let online = p.get("online").and_then(Value::as_i64).unwrap_or(0);
            let max = p.get("max").and_then(Value::as_i64).unwrap_or(0);
            d.population_info = format!("\u{a7}7{online}\u{a7}8/\u{a7}7{max}");
            let sample: Vec<&str> = p.get("sample").and_then(Value::as_array).map(|a| a.iter().filter_map(|e| e.get("name").and_then(Value::as_str)).collect()).unwrap_or_default();
            if !sample.is_empty() {
                let mut list = sample.join("\n");
                if (sample.len() as i64) < online {
                    list.push_str(&format!("\n... and {} more ...", online - sample.len() as i64));
                }
                d.player_list = Some(list);
            }
        }
        _ => d.population_info = "\u{a7}8???".into(),
    }
    match v.get("favicon").and_then(Value::as_str) {
        Some(f) => match f.strip_prefix("data:image/png;base64,") {
            Some(data) => d.icon = Some(data.to_string()),
            None => eprintln!("Invalid server icon (unknown format)"),
        },
        None => d.icon = None,
    }
}

/// `OldServerPinger.ping` plus the error handling around it in `ServerListEntryNormal`.
pub fn ping_server(h: &Handle, lang: &Lang, cancel: &AtomicBool) {
    let ip = h.lock().unwrap().ip.clone();
    let addr = status::parse_address(&ip);
    {
        let mut d = h.lock().unwrap();
        d.motd = "Pinging...".into();
        d.ping = -1;
        d.player_list = None;
    }
    let mut got_status = false;
    let result = status::ping(&addr, cancel, &mut |json| {
        got_status = true;
        apply_status(h, json, lang);
    });
    match result {
        Ok(ms) => h.lock().unwrap().ping = ms as i64,
        Err(PingError::Cancelled) => {}
        Err(PingError::UnknownHost) => {
            let mut d = h.lock().unwrap();
            d.ping = -1;
            d.motd = format!("{DARK_RED}Can't resolve hostname");
        }
        // After the status arrived vanilla keeps what it has and leaves the ping at -1.
        Err(_) if got_status => {}
        Err(e) => {
            eprintln!("Can't ping {ip}: {e:?}");
            {
                let mut d = h.lock().unwrap();
                d.motd = format!("{DARK_RED}Can't connect to server.");
                d.population_info.clear();
            }
            if let Ok(Some(l)) = status::legacy_ping(&addr, cancel) {
                let mut d = h.lock().unwrap();
                d.version = -1;
                d.game_version = l.game_version;
                d.motd = l.motd;
                d.population_info = format!("\u{a7}7{}\u{a7}8/\u{a7}7{}", l.online, l.max);
            }
        }
    }
}

fn mtime(p: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(p).ok()?.modified().ok()
}

/// Migrates remembered pack choices from `server-packs.json` into `servers.dat`
/// (`acceptTextures`): a server still on Prompt takes the store's Enabled/Disabled when the
/// store was written after `servers.dat` (a choice made on the last join, not an older answer
/// that the Edit screen has since reset).
pub fn sync_from_store(dir: &Path) {
    let dat = dir.join("servers.dat");
    let store_file = crate::pack::store_path();
    if mtime(&store_file) <= mtime(&dat) {
        return;
    }
    let store = crate::pack::Store::load();
    let list = ServerList::load(dir);
    let mut changed = false;
    for s in &list.servers {
        let mut d = s.lock().unwrap();
        if d.resource_mode != ResourceMode::Prompt {
            continue;
        }
        let key = crate::args::normalize_server_key(&d.ip);
        match store.mode_for(&key) {
            crate::pack::Mode::Enabled => d.resource_mode = ResourceMode::Enabled,
            crate::pack::Mode::Disabled => d.resource_mode = ResourceMode::Disabled,
            crate::pack::Mode::Prompt => continue,
        }
        changed = true;
    }
    if changed {
        list.save();
    }
}

/// Joining a saved server: its `servers.dat` mode is the truth, so the pack store (which the
/// joined client reads for its cached pack) is brought in line first.
pub fn apply_connect_mode(address: &str, mode: &str) {
    let key = crate::args::normalize_server_key(address);
    let mut store = crate::pack::Store::load();
    let m = crate::pack::Mode::parse(mode);
    let Some(s) = store.servers.get(&key) else { return };
    if s.mode != m {
        let (hash, path) = (s.hash.clone(), s.path.clone());
        store.set(&key, m, &hash, (!path.is_empty()).then(|| Path::new(&path)));
        store.save();
    }
}

/// `ServerList.func_147414_b`: stores a pack decision on the saved server with this address.
#[allow(dead_code)]
pub fn remember_resource_mode(dir: &Path, address: &str, mode: ResourceMode) {
    let list = ServerList::load(dir);
    let key = crate::args::normalize_server_key(address);
    for s in &list.servers {
        let mut d: std::sync::MutexGuard<ServerData> = s.lock().unwrap();
        if crate::args::normalize_server_key(&d.ip) == key {
            d.resource_mode = mode;
        }
    }
    list.save();
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_protocol::frame::{self, Decoder};
    use mc_protocol::packets::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    fn handle(ip: &str) -> Handle {
        Arc::new(Mutex::new(ServerData::new("t", ip, false)))
    }

    #[test]
    fn status_json_fills_the_server_data() {
        let h = handle("x");
        let json = r##"{"version":{"name":"1.8.8","protocol":47},"players":{"max":100,"online":5,"sample":[{"name":"Steve","id":"a"},{"name":"Alex","id":"b"}]},"description":{"text":"Hello ","extra":[{"text":"world","color":"gold"}]},"favicon":"data:image/png;base64,AAAA"}"##;
        apply_status(&h, json, &Lang::default());
        let d = h.lock().unwrap();
        assert_eq!(d.motd, "Hello \u{a7}r\u{a7}6world\u{a7}r");
        assert_eq!((d.version, d.game_version.as_str()), (47, "1.8.8"));
        assert_eq!(d.population_info, "\u{a7}75\u{a7}8/\u{a7}7100");
        assert_eq!(d.player_list.as_deref(), Some("Steve\nAlex\n... and 3 more ..."));
        assert_eq!(d.icon.as_deref(), Some("AAAA"));
    }

    #[test]
    fn missing_sections_use_the_old_server_defaults() {
        let h = handle("x");
        apply_status(&h, r#"{"description":"plain"}"#, &Lang::default());
        let d = h.lock().unwrap();
        assert_eq!((d.version, d.game_version.as_str(), d.population_info.as_str()), (0, "Old", "\u{a7}8???"));
        assert_eq!(d.motd, "plain\u{a7}r");
        assert!(d.icon.is_none());
    }

    #[test]
    fn pinger_fills_the_entry_from_a_status_server() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let (mut dec, mut buf, mut stage) = (Decoder::new(), [0u8; 1024], 0);
            while stage < 3 {
                let n = s.read(&mut buf).unwrap();
                if n == 0 {
                    return;
                }
                dec.feed(&buf[..n]);
                while let Ok(Some(_)) = dec.next_packet() {
                    let body = match stage {
                        1 => Some(StatusClientbound::StatusResponse(StatusResponse { json: r#"{"version":{"name":"1.8.9","protocol":47},"players":{"max":20,"online":0},"description":"mock"}"#.into() }).to_bytes()),
                        2 => {
                            std::thread::sleep(Duration::from_millis(20));
                            Some(StatusClientbound::StatusPong(StatusPong { client_time: 0 }).to_bytes())
                        }
                        _ => None,
                    };
                    if let Some(b) = body {
                        let mut out = Vec::new();
                        frame::encode(&b, -1, &mut out);
                        s.write_all(&out).unwrap();
                    }
                    stage += 1;
                }
            }
        });
        let h = handle(&format!("127.0.0.1:{port}"));
        ping_server(&h, &Lang::default(), &AtomicBool::new(false));
        let d = h.lock().unwrap();
        assert_eq!(d.motd, "mock\u{a7}r");
        assert!(d.ping >= 20, "{}", d.ping);
        assert_eq!(d.population_info, "\u{a7}70\u{a7}8/\u{a7}720");
    }

    #[test]
    fn dead_server_shows_cant_connect() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        let h = handle(&format!("127.0.0.1:{port}"));
        ping_server(&h, &Lang::default(), &AtomicBool::new(false));
        let d = h.lock().unwrap();
        assert_eq!((d.ping, d.motd.as_str()), (-1, "\u{a7}4Can't connect to server."));
    }
}
