//! Command-line arguments.

use crate::*;

pub(crate) struct Args {
    pub(crate) play: bool,
    pub(crate) jar: Option<PathBuf>,
    pub(crate) packs: Vec<PathBuf>,
    pub(crate) world: Option<PathBuf>,
    pub(crate) radius: i32,
    pub(crate) pos: Option<(i32, i32)>,
    pub(crate) screenshot: Option<PathBuf>,
    pub(crate) screen: String,
    pub(crate) bench: Option<f64>,
    pub(crate) seed: Option<i64>,
    /// `host[:port]` of a server to join instead of a local world.
    pub(crate) connect: Option<String>,
    pub(crate) name: String,
    /// Sign in with a Microsoft account for online-mode servers.
    pub(crate) online: bool,
    /// `ServerData.ServerResourceMode` override (`--pack-mode`); persisted per server otherwise.
    pub(crate) pack_mode: Option<pack::Mode>,
}

/// `host:port` with the default port filled in, the key `server-packs.json` stores.
pub(crate) fn normalize_server_key(addr: &str) -> String {
    match addr.rsplit_once(':') {
        Some((h, p)) if p.parse::<u16>().is_ok() => format!("{h}:{p}"),
        _ => format!("{addr}:25565"),
    }
}

pub(crate) fn print_usage() {
    println!(
        "rustmc — Minecraft 1.8.9 client\n\n\
Usage: rustmc [options]\n\n\
  --connect <host[:port]>  join a server (offline-mode by default)\n\
  --name <name>            player name for offline-mode servers (default: Player)\n\
  --online                 sign in with a Microsoft account for online-mode servers\n\
  --jar <1.8.9.jar>        client jar with the assets (auto-detected otherwise)\n\
  --pack <zip|dir>         extra resource pack (repeatable)\n\
  --pack-mode <mode>       prompt | enabled | disabled\n\
  --world <dir>            load a saved world instead of the demo generator\n\
  --seed <seed>            demo world seed\n\
  --radius <n>             demo world radius in chunks\n\
  --pos <x> <z>            demo world centre\n\
  --play                   start the integrated singleplayer server\n\
  --screenshot <png>       save one frame and exit\n\
  --screen <name>          screenshot scene (hud, net, body, …)\n\
  --bench <seconds>        render benchmark\n\n\
Examples:\n\
  rustmc --connect play.example.com --name ASUISSU\n\
  rustmc --connect mc.hypixel.net --online\n\n\
Offline-mode servers accept any --name. Online-mode servers (including Hypixel)\n\
require --online and ignore --name; they kick unauthenticated clients."
    );
}

pub(crate) fn parse_args() -> Args {
    let mut a = Args {
        play: false,
        jar: None,
        packs: vec![],
        world: None,
        radius: 8,
        pos: None,
        screenshot: None,
        screen: "hud".into(),
        bench: None,
        seed: None,
        connect: None,
        name: "Player".into(),
        online: false,
        pack_mode: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            "--play" => a.play = true,
            "--jar" => a.jar = it.next().map(PathBuf::from),
            "--pack" => a.packs.extend(it.next().map(PathBuf::from)),
            "--world" => a.world = it.next().map(PathBuf::from),
            "--screenshot" => a.screenshot = it.next().map(PathBuf::from),
            "--screen" => a.screen = it.next().unwrap_or_default(),
            "--bench" => a.bench = Some(it.next().and_then(|s| s.parse().ok()).unwrap_or(30.0)),
            "--seed" => a.seed = it.next().map(|s| s.parse().unwrap_or_else(|_| java_string_hash(&s) as i64)),
            "--online" => a.online = true,
            "--pack-mode" => a.pack_mode = it.next().map(|s| pack::Mode::parse(&s)),
            "--connect" => a.connect = it.next(),
            "--name" => a.name = it.next().unwrap_or_else(|| "Player".into()),
            "--radius" => a.radius = it.next().and_then(|s| s.parse().ok()).unwrap_or(4),
            "--pos" => {
                let x = it.next().and_then(|s| s.parse().ok());
                let z = it.next().and_then(|s| s.parse().ok());
                if let (Some(x), Some(z)) = (x, z) {
                    a.pos = Some((x, z));
                }
            }
            other => eprintln!("ignoring unknown argument {other}"),
        }
    }
    a
}

pub(crate) fn find_jar() -> Option<PathBuf> {
    let mut candidates = vec![
        PathBuf::from("../MCP-919/jars/versions/1.8.9/1.8.9.jar"),
        PathBuf::from("MCP-919/jars/versions/1.8.9/1.8.9.jar"),
    ];
    if let Ok(exe) = std::env::current_exe() {
        for parent in exe.ancestors().skip(1).take(6) {
            candidates.push(parent.join("MCP-919/jars/versions/1.8.9/1.8.9.jar"));
        }
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        candidates.push(Path::new(&appdata).join(".minecraft/versions/1.8.9/1.8.9.jar"));
    }
    candidates.into_iter().find(|p| p.is_file())
}
