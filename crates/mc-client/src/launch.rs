//! Parse what Prism Launcher passes to a *wrapper command*.
//!
//! Findings (Prism develop @ 18140881, PrismLauncher/PrismLauncher):
//! - With a wrapper set, Prism runs `<wrapper args...> <java> <jvm args> -Djava.library.path=<natives>
//!   -cp <classpath> org.prismlauncher.EntryPoint` (NOT the game main class), see
//!   launcher/minecraft/launch/LauncherPartLaunch.cpp#L133-L146 (`args.prepend(javaPath)`;
//!   `m_process.start(wrapperCommand, wrapperArgs + args)`). Wrapper text is split with
//!   `substituteVariables` (quote paths with spaces); the working dir is the instance's game dir.
//! - All game data goes over the child's **stdin** as `key value` lines (LauncherPartLaunch.cpp#L202
//!   writes the script; `launch\n` is sent later at L222, `abort\n` at L232), built by
//!   `MinecraftInstance::createLaunchScript` (MinecraftInstance.cpp#L812-L905) and read by
//!   libraries/launcher/org/prismlauncher/EntryPoint.java#L90-L125. So the reader must stop at the
//!   `launch` line, not wait for EOF. Lines: `mainClass`, one `param X` per game arg,
//!   `windowParams WxH|maximized`, `serverAddress`/`serverPort` (quick join), `userName`, `sessionId`,
//!   `traits`, `launcher standard`. StandardLauncher.java then appends `--width/--height` (and
//!   `--server/--port`) to the params.
//! - 1.8.9 params come from the meta `minecraftArguments` (meta.prismlauncher.org/v1/net.minecraft/1.8.9.json):
//!   `--username ${auth_player_name} --version ${version_name} --gameDir ${game_directory}
//!   --assetsDir ${assets_root} --assetIndex ${assets_index_name} --uuid ${auth_uuid}
//!   --accessToken ${auth_access_token} --userProperties ${user_properties} --userType ${user_type}`
//!   (token map: MinecraftInstance.cpp#L1060-L1085, L768-L808). mainClass = net.minecraft.client.main.Main.
//! - Assets: `assets_root` = `<prism data>/assets`, index id `1.8` -> `assets/indexes/1.8.json` +
//!   `assets/objects/xx/<hash>` (not virtual; AssetsUtils.cpp#L161).
//! - Client jar: `com.mojang:minecraft:1.8.9:client` = `<prism data>/libraries/com/mojang/minecraft/1.8.9/
//!   minecraft-1.8.9-client.jar`, appended after the libraries (LaunchProfile.cpp#L361-L370), so it is
//!   the last jar of `-cp` (NewLaunch.jar is *prepended*).
//! - Env (MinecraftInstance.cpp#L673-L678): INST_NAME, INST_ID, INST_DIR, INST_MC_DIR, INST_JAVA, INST_JAVA_ARGS.
//! - instance.cfg keys (BaseInstance.cpp#L110-L113): `OverrideCommands=true`, `WrapperCommand=...`.
//!
//! Also accepts the direct form (`... <MainClass> --username X ...`) as a fallback.

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LaunchInfo {
    pub username: String,
    pub uuid: String,
    pub access_token: String,
    pub user_type: String,
    pub version: Option<String>,
    pub game_dir: String,
    pub assets_dir: String,
    pub asset_index: String,
    pub client_jar: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub server: Option<String>,
    pub port: Option<u16>,
}

/// `argv` = everything after the program name. `stdin_script` = lines read from stdin up to `launch`.
pub fn parse(
    argv: &[String],
    stdin_script: Option<&str>,
    env: &impl Fn(&str) -> Option<String>,
) -> Result<LaunchInfo, String> {
    let mut li = LaunchInfo::default();
    let mut ga: Vec<String> = Vec::new();
    let (mut ws, mut sa, mut sp) = (None, None, None);

    // classpath -> client jar; game args follow the main class (direct form)
    let cp_at = argv.iter().position(|a| a == "-cp" || a == "-classpath" || a == "--class-path");
    if let Some(i) = cp_at {
        if let Some(cp) = argv.get(i + 1) {
            let sep = if cp.contains(';') { ';' } else { ':' };
            let jars: Vec<&str> = cp.split(sep).filter(|s| !s.is_empty()).collect();
            li.client_jar = jars
                .iter()
                .find(|j| {
                    let n = j.rsplit(['/', '\\']).next().unwrap_or("");
                    n.starts_with("minecraft-") && n.ends_with("-client.jar")
                })
                .or(jars.last())
                .map(|s| s.to_string());
        }
        ga.extend(argv.iter().skip(i + 3).cloned()); // skip -cp, <cp>, <main class>
    } else {
        ga.extend(argv.iter().cloned());
    }

    if let Some(script) = stdin_script {
        for line in script.lines() {
            let line = line.trim_end_matches('\r');
            match line {
                "launch" => break,
                "abort" => return Err("launch aborted by launcher".into()),
                "" => continue,
                _ => {}
            }
            let (k, v) = line.split_once(' ').ok_or_else(|| format!("bad script line: {line:?}"))?;
            match k {
                "param" => ga.push(v.to_string()),
                "windowParams" => ws = Some(v.to_string()),
                "serverAddress" => sa = Some(v.to_string()),
                "serverPort" => sp = Some(v.to_string()),
                "userName" if li.username.is_empty() => li.username = v.to_string(),
                _ => {}
            }
        }
    }

    let (mut server, mut port) = (sa, sp);
    let mut i = 0;
    while i < ga.len() {
        let v = ga.get(i + 1).cloned();
        let mut take = true;
        match (ga[i].as_str(), v) {
            ("--username", Some(v)) => li.username = v,
            ("--uuid", Some(v)) => li.uuid = v,
            ("--accessToken", Some(v)) => li.access_token = v,
            ("--userType", Some(v)) => li.user_type = v,
            ("--version", Some(v)) => li.version = Some(v),
            ("--gameDir", Some(v)) => li.game_dir = v,
            ("--assetsDir", Some(v)) => li.assets_dir = v,
            ("--assetIndex", Some(v)) => li.asset_index = v,
            ("--width", Some(v)) => li.width = v.parse().ok(),
            ("--height", Some(v)) => li.height = v.parse().ok(),
            ("--server", Some(v)) => server = Some(v),
            ("--port", Some(v)) => port = Some(v),
            _ => take = false,
        }
        i += if take { 2 } else { 1 };
    }

    if let Some(w) = ws {
        if let Some((a, b)) = w.split_once('x') {
            li.width = li.width.or(a.parse().ok());
            li.height = li.height.or(b.parse().ok());
        } // "maximized": leave None
    }
    li.server = server;
    li.port = port.and_then(|p| p.parse().ok());
    if li.game_dir.is_empty() {
        li.game_dir = env("INST_MC_DIR").unwrap_or_default();
    }
    if li.user_type.is_empty() {
        li.user_type = "msa".into();
    }
    for (name, v) in [
        ("username", &li.username),
        ("uuid", &li.uuid),
        ("accessToken", &li.access_token),
        ("gameDir", &li.game_dir),
        ("assetsDir", &li.assets_dir),
        ("assetIndex", &li.asset_index),
    ] {
        if v.is_empty() {
            return Err(format!("missing {name} in launch parameters"));
        }
    }
    Ok(li)
}
