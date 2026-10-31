//! Command-line arguments.

use crate::*;

/// Offline name when `--name` is not given. Set `RUSTMC_DEFAULT_NAME` at build time to use your own.
pub(crate) const DEFAULT_NAME: &str = match option_env!("RUSTMC_DEFAULT_NAME") { Some(n) => n, None => "Player" };

pub(crate) struct Args {
    #[cfg(feature = "singleplayer")]
    pub(crate) play: bool,
    pub(crate) jar: Option<PathBuf>,
    pub(crate) packs: Vec<PathBuf>,
    #[cfg(feature = "singleplayer")]
    pub(crate) world: Option<PathBuf>,
    #[cfg(feature = "singleplayer")]
    pub(crate) radius: i32,
    #[cfg(feature = "singleplayer")]
    pub(crate) pos: Option<(i32, i32)>,
    pub(crate) screenshot: Option<PathBuf>,
    pub(crate) screen: String,
    pub(crate) bench: Option<f64>,
    #[cfg(feature = "singleplayer")]
    pub(crate) seed: Option<i64>,
    /// `host[:port]` of a server to join instead of a local world.
    pub(crate) connect: Option<String>,
    pub(crate) name: String,
    /// Sign in with a Microsoft account for online-mode servers.
    pub(crate) online: bool,
    /// `ServerData.ServerResourceMode` override (`--pack-mode`); persisted per server otherwise.
    pub(crate) pack_mode: Option<pack::Mode>,
    /// Prism: `--assetsDir` / `--assetIndex` for sounds and lang.
    pub(crate) assets_dir: Option<PathBuf>,
    pub(crate) asset_index: String,
    pub(crate) window: Option<(u32, u32)>,
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
  --name <name>            player name for offline-mode servers (default: {DEFAULT_NAME})\n\
  --online                 sign in with a Microsoft account for online-mode servers\n\
  --jar <1.8.9.jar>        client jar with the assets (auto-detected otherwise)\n\
  --pack <zip|dir>         extra resource pack (repeatable)\n\
  --pack-mode <mode>       prompt | enabled | disabled\n\
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
    parse_args_from(std::env::args().skip(1))
}

fn parse_args_from(args: impl Iterator<Item = String>) -> Args {
    let mut a = Args {
        #[cfg(feature = "singleplayer")]
        play: false,
        jar: None,
        packs: vec![],
        #[cfg(feature = "singleplayer")]
        world: None,
        #[cfg(feature = "singleplayer")]
        radius: 8,
        #[cfg(feature = "singleplayer")]
        pos: None,
        screenshot: None,
        screen: "hud".into(),
        bench: None,
        #[cfg(feature = "singleplayer")]
        seed: None,
        connect: None,
        name: DEFAULT_NAME.into(),
        online: false,
        pack_mode: None,
        assets_dir: None,
        asset_index: String::new(),
        window: None,
    };
    let mut it = args;
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            #[cfg(feature = "singleplayer")]
            "--play" => a.play = true,
            "--jar" => a.jar = it.next().map(PathBuf::from),
            "--pack" => a.packs.extend(it.next().map(PathBuf::from)),
            #[cfg(feature = "singleplayer")]
            "--world" => a.world = it.next().map(PathBuf::from),
            "--screenshot" => a.screenshot = it.next().map(PathBuf::from),
            "--screen" => a.screen = it.next().unwrap_or_default(),
            "--bench" => a.bench = Some(it.next().and_then(|s| s.parse().ok()).unwrap_or(30.0)),
            #[cfg(feature = "singleplayer")]
            "--seed" => a.seed = it.next().map(|s| s.parse().unwrap_or_else(|_| java_string_hash(&s) as i64)),
            "--online" => a.online = true,
            "--pack-mode" => a.pack_mode = it.next().map(|s| pack::Mode::parse(&s)),
            "--connect" => a.connect = it.next(),
            "--name" => a.name = it.next().unwrap_or_else(|| DEFAULT_NAME.into()),
            #[cfg(feature = "singleplayer")]
            "--radius" => a.radius = it.next().and_then(|s| s.parse().ok()).unwrap_or(4),
            #[cfg(feature = "singleplayer")]
            "--pos" => {
                let x = it.next().and_then(|s| s.parse().ok());
                let z = it.next().and_then(|s| s.parse().ok());
                if let (Some(x), Some(z)) = (x, z) {
                    a.pos = Some((x, z));
                }
            }
            #[cfg(not(feature = "singleplayer"))]
            "--play" | "--world" | "--seed" | "--radius" | "--pos" => {
                eprintln!("{arg} needs a build with --features singleplayer (singleplayer is disabled)");
            }
            other => eprintln!("ignoring unknown argument {other}"),
        }
    }
    a
}

/// The vanilla launcher's data dir: `%APPDATA%\.minecraft`, `~/Library/Application Support/minecraft`
/// or `~/.minecraft`.
pub(crate) fn minecraft_dir() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        dirs::data_dir().map(|d| d.join(".minecraft"))
    } else if cfg!(target_os = "macos") {
        dirs::data_dir().map(|d| d.join("minecraft"))
    } else {
        dirs::home_dir().map(|d| d.join(".minecraft"))
    }
}

pub(crate) fn find_jar() -> Option<PathBuf> {
    let mut candidates = vec![
        PathBuf::from("MCP-919/jars/versions/1.8.9/1.8.9.jar"),
    ];
    if let Ok(exe) = std::env::current_exe() {
        for parent in exe.ancestors().skip(1).take(6) {
            candidates.push(parent.join("MCP-919/jars/versions/1.8.9/1.8.9.jar"));
        }
    }
    if let Some(d) = minecraft_dir() {
        candidates.push(d.join("versions/1.8.9/1.8.9.jar"));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// Launched by Prism's wrapper command: argv holds `<java> ... org.prismlauncher.EntryPoint`.
pub(crate) fn is_launcher_mode() -> bool {
    std::env::args().skip(1).any(|a| a.ends_with("EntryPoint")) || std::env::var_os("INST_ID").is_some()
}

/// In launcher mode: read Prism's stdin script (up to `launch`), parse it and fill `Args`.
/// Returns the Prism-authenticated session. Exits on a bad script.
pub(crate) fn launcher_args() -> Option<(Args, Option<auth::Session>)> {
    use std::io::BufRead;
    if !is_launcher_mode() {
        return None;
    }
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut script = String::new();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        script.push_str(&line);
        script.push('\n');
        if line == "launch" || line == "abort" {
            break;
        }
    }
    let env = |k: &str| std::env::var(k).ok();
    let li = match launch::parse(&argv, Some(&script), &env) {
        Ok(li) => li,
        Err(e) => {
            eprintln!("rustmc: bad Prism launch: {e}");
            std::process::exit(1);
        }
    };
    // Debug: RUSTMC_LAUNCH_LOG=<file> records what Prism sent, token redacted.
    if let Some(log) = std::env::var_os("RUSTMC_LAUNCH_LOG") {
        let red = |s: &str| s.replace(&li.access_token, "<REDACTED>");
        let msg = format!("argv: {}\nenv INST_*: {:?}\n--- script ---\n{}\n--- parsed ---\n{}\n",
            red(&format!("{argv:?}")),
            std::env::vars().filter(|(k, _)| k.starts_with("INST_")).collect::<Vec<_>>(),
            red(&script),
            red(&format!("{:#?}", li)));
        let _ = std::fs::write(log, msg);
    }
    if let Err(e) = std::env::set_current_dir(&li.game_dir) {
        eprintln!("rustmc: cannot enter game dir {}: {e}", li.game_dir);
    }
    let mut a = parse_args_from(std::iter::empty());
    a.jar = li.client_jar.clone().map(PathBuf::from);
    a.assets_dir = Some(PathBuf::from(&li.assets_dir));
    a.asset_index = li.asset_index.clone();
    a.name = li.username.clone();
    a.window = li.width.zip(li.height);
    a.bench = std::env::var("RUSTMC_BENCH").ok().and_then(|s| s.parse().ok());
    a.connect = li.server.as_ref().map(|s| match li.port {
        Some(p) => format!("{s}:{p}"),
        None => s.clone(),
    });
    let session = auth::Session { name: li.username, uuid: li.uuid, access_token: li.access_token };
    Some((a, Some(session)))
}

/// Initial window size (Prism windowParams).
pub(crate) static WINDOW_SIZE: std::sync::OnceLock<(u32, u32)> = std::sync::OnceLock::new();
