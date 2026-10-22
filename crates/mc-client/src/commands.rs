//! The integrated server's command handler (`CommandHandler` / `ServerCommandManager` and the
//! `Command*` classes), run against the local player, inventory and world.
use crate::level::{GameType, WorldInfo, DIFFICULTY_KEYS};
use crate::{inventory::Catalogue, player::Player};
use mc_core::{BlockPos, Vec3};
use mc_data::Block;
use mc_gui::lang::Lang;
use mc_gui::{ItemStack, Ui};
use mc_world::World;

/// Everything a command can touch.
pub struct Ctx<'a> {
    pub player: &'a mut Player,
    pub world: &'a mut World,
    pub ui: &'a mut Ui,
    pub items: &'a Catalogue,
    pub info: &'a mut WorldInfo,
    /// The player's name (`getName`).
    pub name: &'a str,
}

/// What a command produced: chat lines (already translated, with `§` codes), changed blocks and
/// item entities to spawn at the player (`(stack, fake)`).
#[derive(Default)]
pub struct Outcome {
    pub messages: Vec<String>,
    pub edits: Vec<BlockPos>,
    pub drops: Vec<(ItemStack, bool)>,
    /// The game type changed (abilities must be re-sent).
    pub game_type_changed: bool,
}

/// A translatable message: key plus already-stringified arguments.
#[derive(Debug, Clone)]
struct Msg(String, Vec<String>);

fn msg(key: &str, args: Vec<String>) -> Msg {
    Msg(key.into(), args)
}

#[derive(Debug)]
enum Error {
    /// `WrongUsageException`
    Usage(&'static str),
    /// `CommandException` and subclasses
    Cmd(Msg),
    /// `CommandNotFoundException`
    NotFound,
}

fn err<T>(key: &str, args: Vec<String>) -> Result<T, Error> {
    Err(Error::Cmd(msg(key, args)))
}

type Res = Result<(), Error>;

struct Command {
    name: &'static str,
    aliases: &'static [&'static str],
    usage: &'static str,
    run: fn(&mut Run, &[&str]) -> Res,
}

/// The commands registered by `ServerCommandManager` that this client implements, sorted by name.
const COMMANDS: &[Command] = &[
    Command { name: "clear", aliases: &[], usage: "commands.clear.usage", run: clear },
    Command { name: "defaultgamemode", aliases: &[], usage: "commands.defaultgamemode.usage", run: default_game_mode },
    Command { name: "difficulty", aliases: &[], usage: "commands.difficulty.usage", run: difficulty },
    Command { name: "fill", aliases: &[], usage: "commands.fill.usage", run: fill },
    Command { name: "gamemode", aliases: &[], usage: "commands.gamemode.usage", run: game_mode },
    Command { name: "gamerule", aliases: &[], usage: "commands.gamerule.usage", run: game_rule },
    Command { name: "give", aliases: &[], usage: "commands.give.usage", run: give },
    Command { name: "help", aliases: &["?"], usage: "commands.help.usage", run: help },
    Command { name: "kill", aliases: &[], usage: "commands.kill.usage", run: kill },
    Command { name: "me", aliases: &[], usage: "commands.me.usage", run: emote },
    Command { name: "say", aliases: &[], usage: "commands.say.usage", run: say },
    Command { name: "seed", aliases: &[], usage: "commands.seed.usage", run: seed },
    Command { name: "setblock", aliases: &[], usage: "commands.setblock.usage", run: set_block },
    Command { name: "setworldspawn", aliases: &[], usage: "commands.setworldspawn.usage", run: set_world_spawn },
    Command { name: "spawnpoint", aliases: &[], usage: "commands.spawnpoint.usage", run: spawn_point },
    Command { name: "tell", aliases: &["w", "msg"], usage: "commands.message.usage", run: tell },
    Command { name: "testforblock", aliases: &[], usage: "commands.testforblock.usage", run: test_for_block },
    Command { name: "time", aliases: &[], usage: "commands.time.usage", run: time },
    Command { name: "toggledownfall", aliases: &[], usage: "commands.downfall.usage", run: toggle_downfall },
    Command { name: "tp", aliases: &[], usage: "commands.tp.usage", run: teleport },
    Command { name: "weather", aliases: &[], usage: "commands.weather.usage", run: weather },
];

fn lookup(name: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.name == name).or_else(|| COMMANDS.iter().find(|c| c.aliases.contains(&name)))
}

/// `MinecraftServer.getTabCompletions` for the chat line before the cursor.
pub fn complete(input: &str, _lang: &Lang, looking_at: Option<BlockPos>) -> Vec<String> {
    let players = ["Player"];
    let Some(cmd) = input.strip_prefix('/') else {
        let last = input.rsplit(' ').next().unwrap_or("");
        return players.iter().filter(|p| starts_with_ignore_case(p, last)).map(|s| s.to_string()).collect();
    };
    let parts: Vec<&str> = cmd.split(' ').collect();
    if parts.len() == 1 {
        return complete_name(parts[0]).into_iter().map(|s| format!("/{s}")).collect();
    }
    let Some(c) = lookup(parts[0]) else { return Vec::new() };
    let args = &parts[1..];
    let n = args.len();
    let last = args[n - 1];
    let words = |opts: &[&str]| -> Vec<String> { opts.iter().filter(|o| starts_with_ignore_case(o, last)).map(|s| s.to_string()).collect() };
    let registry = |names: Vec<&'static str>| -> Vec<String> {
        // getListOfStringsMatchingLastWord over ResourceLocations: full names, then paths.
        let full: Vec<String> = names.iter().filter(|o| starts_with_ignore_case(o, last)).map(|s| s.to_string()).collect();
        if !full.is_empty() {
            return full;
        }
        names.iter().filter(|o| starts_with_ignore_case(o.trim_start_matches("minecraft:"), last)).map(|s| s.to_string()).collect()
    };
    let coord = |start: usize| -> Vec<String> {
        // func_175771_a: the looked-at block's coordinate for the current axis.
        let Some(p) = looking_at else { return Vec::new() };
        match n as i32 - 1 - start as i32 {
            0 => vec![p.x.to_string()],
            1 => vec![p.y.to_string()],
            2 => vec![p.z.to_string()],
            _ => Vec::new(),
        }
    };
    let blocks = || registry(Block::all().iter().map(|b| b.name).collect());
    let items = || registry(mc_data::item::Item::all().iter().map(|i| i.name).collect());
    match c.name {
        "help" if n == 1 => {
            let mut v: Vec<String> = COMMANDS.iter().flat_map(|c| std::iter::once(c.name).chain(c.aliases.iter().copied())).filter(|o| starts_with_ignore_case(o, last)).map(String::from).collect();
            v.sort();
            v
        }
        "gamemode" if n == 1 => words(&["survival", "creative", "adventure", "spectator"]),
        "defaultgamemode" if n == 1 => words(&["survival", "creative", "adventure", "spectator"]),
        "difficulty" if n == 1 => words(&["peaceful", "easy", "normal", "hard"]),
        "time" if n == 1 => words(&["set", "add", "query"]),
        "time" if n == 2 && args[0] == "set" => words(&["day", "night"]),
        "time" if n == 2 && args[0] == "query" => words(&["daytime", "gametime"]),
        "weather" if n == 1 => words(&["clear", "rain", "thunder"]),
        "gamerule" if n == 1 => {
            let rules = crate::level::GameRules::default();
            words(&rules.names())
        }
        "gamerule" if n == 2 && crate::level::GameRules::default().is_boolean(args[0]) => words(&["true", "false"]),
        "give" | "clear" if n == 2 => items(),
        "setblock" | "testforblock" if n <= 3 => coord(0),
        "setblock" | "testforblock" if n == 4 => blocks(),
        "setblock" if n == 6 => words(&["replace", "destroy", "keep"]),
        "fill" if n <= 3 => coord(0),
        "fill" if n <= 6 => coord(3),
        "fill" if n == 7 => blocks(),
        "fill" if n == 9 => words(&["replace", "destroy", "keep", "hollow", "outline"]),
        "fill" if n == 10 && args[8] == "replace" => blocks(),
        "setworldspawn" if n <= 3 => coord(0),
        "spawnpoint" if n == 1 => words(&players),
        "spawnpoint" if n <= 4 => coord(1),
        "gamemode" | "xp" if n == 2 => words(&players),
        "give" | "clear" | "kill" | "tell" | "me" | "say" if n == 1 => words(&players),
        "tp" if n == 1 || n == 2 => words(&players),
        "me" | "say" | "tell" => words(&players),
        _ => Vec::new(),
    }
}

/// Command names and aliases starting with `prefix` (case-insensitive), for tab completion.
pub fn complete_name(prefix: &str) -> Vec<String> {
    let mut v: Vec<String> = COMMANDS
        .iter()
        .flat_map(|c| std::iter::once(c.name).chain(c.aliases.iter().copied()))
        .filter(|n| starts_with_ignore_case(n, prefix))
        .map(String::from)
        .collect();
    v.sort();
    v
}

fn starts_with_ignore_case(s: &str, prefix: &str) -> bool {
    s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix)
}

struct Run<'c, 'a> {
    c: &'c mut Ctx<'a>,
    out: Outcome,
}

impl Run<'_, '_> {
    fn lang(&self) -> &Lang {
        &self.c.ui.lang
    }
    fn tr(&self, m: &Msg) -> String {
        let args: Vec<&str> = m.1.iter().map(String::as_str).collect();
        self.lang().format(&m.0, &args)
    }
    /// `addChatMessage(new ChatComponentTranslation(...))`
    fn send(&mut self, m: Msg) {
        let t = self.tr(&m);
        self.out.messages.push(t);
    }
    fn send_raw(&mut self, s: String) {
        self.out.messages.push(s);
    }
    /// `CommandBase.notifyOperators`: the sender sees the message if `sendCommandFeedback`.
    fn notify(&mut self, key: &str, args: Vec<String>) {
        if self.c.info.rules.get_bool("sendCommandFeedback") {
            self.send(msg(key, args));
        }
    }
    fn t(&self, key: &str) -> String {
        self.lang().get(key)
    }
    /// `EntityPlayerMP.getPosition`: `new BlockPos(posX, posY + 0.5, posZ)`.
    fn sender_pos(&self) -> BlockPos {
        let p = self.c.player.pos;
        BlockPos::new(p.x.floor() as i32, (p.y + 0.5).floor() as i32, p.z.floor() as i32)
    }
    /// `getPlayer`: selectors and the (only) player's name.
    fn player(&self, arg: &str) -> Result<(), Error> {
        if self.matches_player(arg) {
            Ok(())
        } else {
            err("commands.generic.player.notFound", vec![])
        }
    }
    fn matches_player(&self, arg: &str) -> bool {
        if arg.eq_ignore_ascii_case(self.c.name) {
            return true;
        }
        match selector(arg) {
            Some((kind, args)) => {
                let typed = args.iter().find(|(k, _)| k == "type").map(|(_, v)| v.as_str());
                let ok_type = match kind {
                    'e' => typed.is_none_or(|t| t == "Player" || t == "!" || (t.starts_with('!') && t != "!Player")),
                    _ => true,
                };
                let named = args.iter().find(|(k, _)| k == "name").map(|(_, v)| v.as_str());
                let ok_name = named.is_none_or(|n| match n.strip_prefix('!') {
                    Some(n) => n != self.c.name,
                    None => n == self.c.name,
                });
                ok_type && ok_name
            }
            None => false,
        }
    }
    /// `getEntity`: the player, or the vanilla errors for anything else.
    fn entity(&self, arg: &str) -> Result<(), Error> {
        if self.matches_player(arg) {
            return Ok(());
        }
        if selector(arg).is_some() || is_uuid(arg) {
            err("commands.generic.entity.notFound", vec![])
        } else {
            err("commands.generic.entity.invalidUuid", vec![])
        }
    }
    fn parse_block_pos(&self, args: &[&str], i: usize, center: bool) -> Result<BlockPos, Error> {
        let b = self.sender_pos();
        Ok(BlockPos::new(
            parse_rel(b.x as f64, args[i], -30_000_000, 30_000_000, center)?.floor() as i32,
            parse_rel(b.y as f64, args[i + 1], 0, 256, false)?.floor() as i32,
            parse_rel(b.z as f64, args[i + 2], -30_000_000, 30_000_000, center)?.floor() as i32,
        ))
    }
    fn block_name(&self, b: &Block) -> String {
        self.c
            .items
            .items
            .iter()
            .find(|i| i.block == b.id && i.id == b.id)
            .map_or_else(|| self.t(&format!("tile.{}.name", b.name.trim_start_matches("minecraft:"))), |i| i.title.clone())
    }
}

/// `@p`/`@a`/`@r`/`@e` with `[k=v,...]` arguments.
fn selector(s: &str) -> Option<(char, Vec<(String, String)>)> {
    let mut ch = s.chars();
    if ch.next() != Some('@') {
        return None;
    }
    let kind = ch.next().filter(|c| matches!(c, 'p' | 'a' | 'r' | 'e'))?;
    let rest = &s[2..];
    if rest.is_empty() {
        return Some((kind, vec![]));
    }
    let inner = rest.strip_prefix('[')?.strip_suffix(']')?;
    let args = inner
        .split(',')
        .filter(|p| !p.is_empty())
        .map(|p| p.split_once('=').map_or((String::new(), p.to_string()), |(k, v)| (k.to_string(), v.to_string())))
        .collect();
    Some((kind, args))
}

fn is_uuid(s: &str) -> bool {
    // UUID.fromString accepts five dash-separated hex groups.
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 5 && parts.iter().all(|p| !p.is_empty() && p.len() <= 16 && p.chars().all(|c| c.is_ascii_hexdigit()))
}

/// `Double.toString`
pub fn java_double(d: f64) -> String {
    if d.is_nan() {
        return "NaN".into();
    }
    if d.is_infinite() {
        return if d > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if d == 0.0 {
        return if d.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    let a = d.abs();
    if (1e-3..1e7).contains(&a) {
        let s = format!("{d}");
        if s.contains('.') {
            s
        } else {
            s + ".0"
        }
    } else {
        let s = format!("{d:e}");
        let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
        let m = if m.contains('.') { m.to_string() } else { format!("{m}.0") };
        format!("{m}E{e}")
    }
}

/// `Integer.parseInt`
fn java_int(s: &str) -> Option<i32> {
    s.parse::<i32>().ok()
}

fn parse_int(s: &str) -> Result<i32, Error> {
    java_int(s).ok_or_else(|| Error::Cmd(msg("commands.generic.num.invalid", vec![s.into()])))
}

fn parse_int_range(s: &str, min: i32, max: i32) -> Result<i32, Error> {
    let i = parse_int(s)?;
    if i < min {
        err("commands.generic.num.tooSmall", vec![i.to_string(), min.to_string()])
    } else if i > max {
        err("commands.generic.num.tooBig", vec![i.to_string(), max.to_string()])
    } else {
        Ok(i)
    }
}

/// `Double.parseDouble` + finiteness check.
fn parse_double(s: &str) -> Result<f64, Error> {
    let t = s.trim_matches(|c: char| c <= ' ');
    let t = t.strip_suffix(['d', 'D', 'f', 'F']).unwrap_or(t);
    match t.parse::<f64>() {
        Ok(d) if d.is_finite() => Ok(d),
        _ => err("commands.generic.num.invalid", vec![s.into()]),
    }
}

/// `CommandBase.parseCoordinate` / `parseDouble(base, input, min, max, centerBlock)`:
/// returns (absolute value, amount, relative).
fn parse_coord(base: f64, s: &str, min: i32, max: i32, center: bool) -> Result<(f64, f64, bool), Error> {
    let rel = s.starts_with('~');
    if rel && base.is_nan() {
        return err("commands.generic.num.invalid", vec![java_double(base)]);
    }
    let mut d0 = 0.0;
    if !rel || s.len() > 1 {
        let has_dot = s.contains('.');
        let t = if rel { &s[1..] } else { s };
        d0 += parse_double(t)?;
        if !has_dot && !rel && center {
            d0 += 0.5;
        }
    }
    if min != 0 || max != 0 {
        if d0 < min as f64 {
            return err("commands.generic.double.tooSmall", vec![java_double(d0), min.to_string()]);
        }
        if d0 > max as f64 {
            return err("commands.generic.double.tooBig", vec![java_double(d0), max.to_string()]);
        }
    }
    Ok((d0 + if rel { base } else { 0.0 }, d0, rel))
}

/// `parseDouble(base, input, min, max, centerBlock)`: the range check applies to the result.
fn parse_rel(base: f64, s: &str, min: i32, max: i32, center: bool) -> Result<f64, Error> {
    let rel = s.starts_with('~');
    if rel && base.is_nan() {
        return err("commands.generic.num.invalid", vec![java_double(base)]);
    }
    let mut d0 = if rel { base } else { 0.0 };
    if !rel || s.len() > 1 {
        let has_dot = s.contains('.');
        let t = if rel { &s[1..] } else { s };
        d0 += parse_double(t)?;
        if !has_dot && !rel && center {
            d0 += 0.5;
        }
    }
    if min != 0 || max != 0 {
        if d0 < min as f64 {
            return err("commands.generic.double.tooSmall", vec![java_double(d0), min.to_string()]);
        }
        if d0 > max as f64 {
            return err("commands.generic.double.tooBig", vec![java_double(d0), max.to_string()]);
        }
    }
    Ok(d0)
}

/// `new ResourceLocation(id).toString()`
fn resource_location(id: &str) -> String {
    match id.split_once(':') {
        Some((d, p)) => format!("{}:{p}", if d.is_empty() { "minecraft".to_string() } else { d.to_lowercase() }),
        None => format!("minecraft:{id}"),
    }
}

fn block_by_text(id: &str) -> Result<&'static Block, Error> {
    let rl = resource_location(id);
    Block::by_name(&rl).ok_or_else(|| Error::Cmd(msg("commands.give.block.notFound", vec![rl])))
}

/// `CommandBase.buildString`
fn build_string(args: &[&str], start: usize) -> String {
    args[start.min(args.len())..].join(" ")
}

fn wrap_degrees(mut f: f32) -> f32 {
    f %= 360.0;
    if f >= 180.0 {
        f -= 360.0;
    }
    if f < -180.0 {
        f += 360.0;
    }
    f
}

/// `StringUtils.normalizeSpace` as `NetHandlerPlayServer.processChatMessage` applies it.
pub fn normalize_space(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `CommandHandler.executeCommand`
pub fn execute(text: &str, c: &mut Ctx) -> Outcome {
    let raw = text.trim();
    let raw = raw.strip_prefix('/').unwrap_or(raw);
    // String.split(" "): trailing empty strings are dropped.
    let mut parts: Vec<&str> = raw.split(' ').collect();
    while parts.len() > 1 && parts.last() == Some(&"") {
        parts.pop();
    }
    let name = parts[0];
    let args = &parts[1..];
    let mut r = Run { c, out: Outcome::default() };
    match lookup(name) {
        None => {
            let t = r.t("commands.generic.notFound");
            r.send_raw(format!("\u{a7}c{t}"));
        }
        Some(cmd) => {
            if let Err(e) = (cmd.run)(&mut r, args) {
                let line = match e {
                    Error::Usage(key) => {
                        let usage = r.t(key);
                        r.lang().format("commands.generic.usage", &[&usage])
                    }
                    Error::Cmd(m) => r.tr(&m),
                    Error::NotFound => r.t("commands.generic.notFound"),
                };
                r.send_raw(format!("\u{a7}c{line}"));
            }
        }
    }
    r.out
}

fn help(r: &mut Run, args: &[&str]) -> Res {
    let list = COMMANDS;
    let j = (list.len() as i32 - 1) / 7;
    let k = if args.is_empty() {
        0
    } else {
        match parse_int_range(args[0], 1, j + 1) {
            Ok(n) => n - 1,
            Err(e) => {
                if let Some(cmd) = lookup(args[0]) {
                    return Err(Error::Usage(cmd.usage));
                }
                if java_int(args[0]).unwrap_or(-1) != -1 {
                    return Err(e);
                }
                return Err(Error::NotFound);
            }
        }
    };
    let l = ((k + 1) * 7).min(list.len() as i32);
    let header = r.tr(&msg("commands.help.header", vec![(k + 1).to_string(), (j + 1).to_string()]));
    r.send_raw(format!("\u{a7}2{header}"));
    for cmd in &list[(k * 7) as usize..l as usize] {
        let u = r.t(cmd.usage);
        r.send_raw(u);
    }
    if k == 0 {
        let f = r.t("commands.help.footer");
        r.send_raw(format!("\u{a7}a{f}"));
    }
    Ok(())
}

/// `CommandGameMode.getGameModeFromCommand`
fn game_mode_arg(s: &str) -> Result<GameType, Error> {
    Ok(if s.eq_ignore_ascii_case("survival") || s.eq_ignore_ascii_case("s") {
        GameType::Survival
    } else if s.eq_ignore_ascii_case("creative") || s.eq_ignore_ascii_case("c") {
        GameType::Creative
    } else if s.eq_ignore_ascii_case("adventure") || s.eq_ignore_ascii_case("a") {
        GameType::Adventure
    } else if s.eq_ignore_ascii_case("spectator") || s.eq_ignore_ascii_case("sp") {
        GameType::Spectator
    } else {
        GameType::by_id(parse_int_range(s, 0, 3)?)
    })
}

fn game_mode(r: &mut Run, args: &[&str]) -> Res {
    if args.is_empty() {
        return Err(Error::Usage("commands.gamemode.usage"));
    }
    let g = game_mode_arg(args[0])?;
    if args.len() >= 2 {
        r.player(args[1])?;
    }
    r.c.player.set_game_type(g);
    r.c.player.fall_distance = 0.0;
    r.out.game_type_changed = true;
    if r.c.info.rules.get_bool("sendCommandFeedback") {
        r.send(msg("gameMode.changed", vec![]));
    }
    let mode = r.t(&format!("gameMode.{}", g.name()));
    r.notify("commands.gamemode.success.self", vec![mode]);
    Ok(())
}

fn default_game_mode(r: &mut Run, args: &[&str]) -> Res {
    if args.is_empty() {
        return Err(Error::Usage("commands.defaultgamemode.usage"));
    }
    let g = game_mode_arg(args[0])?;
    r.c.info.game_type = g;
    let mode = r.t(&format!("gameMode.{}", g.name()));
    r.notify("commands.defaultgamemode.success", vec![mode]);
    Ok(())
}

fn difficulty(r: &mut Run, args: &[&str]) -> Res {
    if args.is_empty() {
        return Err(Error::Usage("commands.difficulty.usage"));
    }
    let s = args[0];
    let d = if s.eq_ignore_ascii_case("peaceful") || s.eq_ignore_ascii_case("p") {
        0
    } else if s.eq_ignore_ascii_case("easy") || s.eq_ignore_ascii_case("e") {
        1
    } else if s.eq_ignore_ascii_case("normal") || s.eq_ignore_ascii_case("n") {
        2
    } else if s.eq_ignore_ascii_case("hard") || s.eq_ignore_ascii_case("h") {
        3
    } else {
        parse_int_range(s, 0, 3)?
    };
    r.c.info.difficulty = d as u8;
    let name = r.t(DIFFICULTY_KEYS[d as usize]);
    r.notify("commands.difficulty.success", vec![name]);
    Ok(())
}

fn time(r: &mut Run, args: &[&str]) -> Res {
    if args.len() > 1 {
        match args[0] {
            "set" => {
                let l = match args[1] {
                    "day" => 1000,
                    "night" => 13000,
                    s => parse_int_range(s, 0, i32::MAX)?,
                };
                r.c.info.time = l as i64;
                r.notify("commands.time.set", vec![l.to_string()]);
                return Ok(());
            }
            "add" => {
                let k = parse_int_range(args[1], 0, i32::MAX)?;
                r.c.info.time += k as i64;
                r.notify("commands.time.added", vec![k.to_string()]);
                return Ok(());
            }
            "query" => {
                let v = match args[1] {
                    "daytime" => Some(r.c.info.time),
                    "gametime" => Some(r.c.info.total_time),
                    _ => None,
                };
                if let Some(v) = v {
                    let j = (v % 2147483647) as i32;
                    r.notify("commands.time.query", vec![j.to_string()]);
                    return Ok(());
                }
            }
            _ => {}
        }
    }
    Err(Error::Usage("commands.time.usage"))
}

fn weather(r: &mut Run, args: &[&str]) -> Res {
    if args.is_empty() || args.len() > 2 {
        return Err(Error::Usage("commands.weather.usage"));
    }
    let mut i = (300 + (rand_u32() % 600) as i32) * 20;
    if args.len() >= 2 {
        i = parse_int_range(args[1], 1, 1_000_000)? * 20;
    }
    let w = &mut *r.c.info;
    let key = if args[0].eq_ignore_ascii_case("clear") {
        (w.clean_weather_time, w.rain_time, w.thunder_time, w.raining, w.thundering) = (i, 0, 0, false, false);
        "commands.weather.clear"
    } else if args[0].eq_ignore_ascii_case("rain") {
        (w.clean_weather_time, w.rain_time, w.thunder_time, w.raining, w.thundering) = (0, i, i, true, false);
        "commands.weather.rain"
    } else if args[0].eq_ignore_ascii_case("thunder") {
        (w.clean_weather_time, w.rain_time, w.thunder_time, w.raining, w.thundering) = (0, i, i, true, true);
        "commands.weather.thunder"
    } else {
        return Err(Error::Usage("commands.weather.usage"));
    };
    r.notify(key, vec![]);
    Ok(())
}

/// `new Random().nextInt` stand-in.
fn rand_u32() -> u32 {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    (t.as_nanos() as u64).wrapping_mul(0x9E3779B97F4A7C15).rotate_left(17) as u32
}

fn toggle_downfall(r: &mut Run, _args: &[&str]) -> Res {
    r.c.info.raining = !r.c.info.raining;
    r.notify("commands.downfall.success", vec![]);
    Ok(())
}

fn kill(r: &mut Run, args: &[&str]) -> Res {
    if let Some(a) = args.first() {
        r.entity(a)?;
    }
    // onKillCommand: attackEntityFrom(outOfWorld, MAX_VALUE) kills even creative players.
    if !r.c.player.dead {
        r.c.player.dead = true;
        if r.c.info.rules.get_bool("showDeathMessages") {
            let name = r.c.name.to_string();
            r.send(msg("death.attack.outOfWorld", vec![name]));
        }
    }
    let name = r.c.name.to_string();
    r.notify("commands.kill.successful", vec![name]);
    Ok(())
}

fn teleport(r: &mut Run, args: &[&str]) -> Res {
    if args.is_empty() {
        return Err(Error::Usage("commands.tp.usage"));
    }
    let mut i = 0;
    if matches!(args.len(), 2 | 4 | 6) {
        r.entity(args[0])?;
        i = 1;
    }
    let name = r.c.name.to_string();
    if args.len() != 1 && args.len() != 2 {
        if args.len() < i + 3 {
            return Err(Error::Usage("commands.tp.usage"));
        }
        let p = &*r.c.player;
        let x = parse_coord(p.pos.x, args[i], -30_000_000, 30_000_000, true)?;
        let y = parse_coord(p.pos.y, args[i + 1], 0, 0, false)?;
        let z = parse_coord(p.pos.z, args[i + 2], -30_000_000, 30_000_000, true)?;
        let yaw = parse_coord(p.yaw as f64, args.get(i + 3).copied().unwrap_or("~"), -30_000_000, 30_000_000, false)?;
        let pitch = parse_coord(p.pitch as f64, args.get(i + 4).copied().unwrap_or("~"), -30_000_000, 30_000_000, false)?;
        let mut f = yaw.1 as f32;
        if !yaw.2 {
            f = wrap_degrees(f);
        }
        let mut f1 = pitch.1 as f32;
        if !pitch.2 {
            f1 = wrap_degrees(f1);
        }
        if !(-90.0..=90.0).contains(&f1) {
            f1 = wrap_degrees(180.0 - f1);
            f = wrap_degrees(f + 180.0);
        }
        // NetHandlerPlayClient.handlePlayerPosLook: relative fields add to the current values.
        let pl = &mut *r.c.player;
        let pos = Vec3::new(
            if x.2 { pl.pos.x + x.1 } else { x.1 },
            if y.2 { pl.pos.y + y.1 } else { y.1 },
            if z.2 { pl.pos.z + z.1 } else { z.1 },
        );
        let new_yaw = if yaw.2 { pl.yaw + f } else { f };
        let new_pitch = if pitch.2 { pl.pitch + f1 } else { f1 };
        pl.teleport(pos);
        pl.yaw = new_yaw;
        pl.pitch = new_pitch;
        pl.prev_yaw = new_yaw;
        pl.prev_pitch = new_pitch;
        r.notify("commands.tp.success.coordinates", vec![name, java_double(x.0), java_double(y.0), java_double(z.0)]);
    } else {
        r.entity(args[args.len() - 1])?;
        // Teleporting onto yourself keeps position and angles.
        r.notify("commands.tp.success", vec![name.clone(), name]);
    }
    Ok(())
}

/// `EnumRarity` colour of `ItemStack.getChatComponent`.
fn rarity_color(id: u16, meta: u16) -> Option<char> {
    match id {
        322 => Some(if meta > 0 { 'd' } else { 'b' }),
        2256..=2267 => Some('b'),
        _ => None,
    }
}

fn give(r: &mut Run, args: &[&str]) -> Res {
    if args.len() < 2 {
        return Err(Error::Usage("commands.give.usage"));
    }
    r.player(args[0])?;
    let rl = resource_location(args[1]);
    let Some(reg) = mc_data::item::Item::by_name(&rl) else {
        return err("commands.give.item.notFound", vec![rl]);
    };
    let i = if args.len() >= 3 { parse_int_range(args[2], 1, 64)? } else { 1 };
    let j = if args.len() >= 4 { parse_int(args[3])? } else { 0 };
    let Some(item) = r.c.items.index_of(reg.id, j.clamp(0, u16::MAX as i32) as u16) else {
        return err("commands.give.item.notFound", vec![rl]);
    };
    let def = &r.c.items.items[item];
    let (title, color) = (def.title.clone(), rarity_color(def.id, def.meta));
    let left = add_to_inventory(r.c.ui, r.c.items, item, i as u8, r.c.player.game_type == GameType::Creative);
    if left == 0 {
        // Fully added: a one-tick "fake" item for the pickup animation.
        r.out.drops.push((ItemStack { item, count: 1 }, true));
    } else {
        r.out.drops.push((ItemStack { item, count: left }, false));
    }
    let component = match color {
        Some(c) => format!("\u{a7}{c}[{title}]\u{a7}r"),
        None => format!("[{title}]"),
    };
    let name = r.c.name.to_string();
    r.notify("commands.give.success", vec![component, i.to_string(), name]);
    Ok(())
}

/// `InventoryPlayer.addItemStackToInventory`; returns the count that didn't fit.
pub fn add_to_inventory(ui: &mut Ui, items: &Catalogue, item: usize, mut count: u8, creative: bool) -> u8 {
    let start = count;
    let max = items.items[item].max.max(1);
    loop {
        let slot = ui.inventory.iter().position(|s| s.is_some_and(|s| s.item == item && s.count < max)).or_else(|| ui.inventory.iter().position(Option::is_none));
        let Some(slot) = slot else { break };
        let have = ui.inventory[slot].map_or(0, |s| s.count);
        let n = count.min(max - have);
        ui.inventory[slot] = Some(ItemStack { item, count: have + n });
        count -= n;
        if count == 0 {
            break;
        }
    }
    if count == start && creative {
        return 0;
    }
    count
}

fn clear(r: &mut Run, args: &[&str]) -> Res {
    if let Some(a) = args.first() {
        r.player(a)?;
    }
    let item = if args.len() >= 2 {
        let rl = resource_location(args[1]);
        Some(mc_data::item::Item::by_name(&rl).ok_or_else(|| Error::Cmd(msg("commands.give.item.notFound", vec![rl])))?.id)
    } else {
        None
    };
    let meta = if args.len() >= 3 { parse_int_range(args[2], -1, i32::MAX)? } else { -1 };
    let max = if args.len() >= 4 { parse_int_range(args[3], -1, i32::MAX)? } else { -1 };
    let items = r.c.items;
    let ui = &mut *r.c.ui;
    let mut i = 0;
    let slots = ui.inventory.iter_mut().chain(ui.armor.iter_mut());
    for slot in slots {
        let Some(s) = *slot else { continue };
        let def = &items.items[s.item];
        if item.is_some_and(|id| def.id != id) || (meta > -1 && def.meta as i32 != meta) {
            continue;
        }
        let k = if max <= 0 { s.count as i32 } else { (max - i).min(s.count as i32) };
        i += k;
        if max != 0 {
            let n = s.count as i32 - k;
            *slot = (n > 0).then_some(ItemStack { count: n as u8, ..s });
            if max > 0 && i >= max {
                break;
            }
        }
    }
    let name = r.c.name.to_string();
    if i == 0 {
        return err("commands.clear.failure", vec![name]);
    }
    if max == 0 {
        r.send(msg("commands.clear.testing", vec![name, i.to_string()]));
    } else {
        r.notify("commands.clear.success", vec![name, i.to_string()]);
    }
    Ok(())
}

fn set_block(r: &mut Run, args: &[&str]) -> Res {
    if args.len() < 4 {
        return Err(Error::Usage("commands.setblock.usage"));
    }
    let pos = r.parse_block_pos(args, 0, false)?;
    let block = block_by_text(args[3])?;
    let meta = if args.len() >= 5 { parse_int_range(args[4], 0, 15)? } else { 0 };
    if !r.c.world.is_block_loaded(pos) {
        return err("commands.setblock.outOfWorld", vec![]);
    }
    if args.len() >= 6 {
        if args[5] == "destroy" {
            if r.c.world.set_block_state(pos, mc_data::StateId::AIR).is_some() {
                r.out.edits.push(pos);
            }
            if block.id == 0 {
                r.notify("commands.setblock.success", vec![]);
                return Ok(());
            }
        } else if args[5] == "keep" && !r.c.world.is_air(pos) {
            return err("commands.setblock.noChange", vec![]);
        }
    }
    if let Some(c) = r.c.world.chunks.get_mut(&(pos.x >> 4, pos.z >> 4)) {
        if c.tile_entities.remove(&pos).is_some() {
            r.c.world.set_block_state(pos, mc_data::StateId::AIR);
        }
    }
    let state = block.state_from_meta(meta as u8).unwrap_or(block.default_state);
    if r.c.world.set_block_state(pos, state).is_none() {
        return err("commands.setblock.noChange", vec![]);
    }
    r.out.edits.push(pos);
    r.notify("commands.setblock.success", vec![]);
    Ok(())
}

fn fill(r: &mut Run, args: &[&str]) -> Res {
    if args.len() < 7 {
        return Err(Error::Usage("commands.fill.usage"));
    }
    let a = r.parse_block_pos(args, 0, false)?;
    let b = r.parse_block_pos(args, 3, false)?;
    let block = block_by_text(args[6])?;
    let meta = if args.len() >= 8 { parse_int_range(args[7], 0, 15)? } else { 0 };
    let lo = BlockPos::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z));
    let hi = BlockPos::new(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z));
    let j = (hi.x - lo.x + 1).wrapping_mul(hi.y - lo.y + 1).wrapping_mul(hi.z - lo.z + 1);
    if j > 32768 {
        return err("commands.fill.tooManyBlocks", vec![j.to_string(), "32768".into()]);
    }
    if lo.y < 0 || hi.y >= 256 {
        return err("commands.fill.outOfWorld", vec![]);
    }
    let mut k = lo.z;
    while k < hi.z + 16 {
        let mut l = lo.x;
        while l < hi.x + 16 {
            if !r.c.world.is_block_loaded(BlockPos::new(l, hi.y - lo.y, k)) {
                return err("commands.fill.outOfWorld", vec![]);
            }
            l += 16;
        }
        k += 16;
    }
    let state = block.state_from_meta(meta as u8).unwrap_or(block.default_state);
    let mode = args.get(8).copied();
    let filter = if mode == Some("replace") && args.len() > 9 { Some(block_by_text(args[9])?.id) } else { None };
    let filter_meta = if mode == Some("replace") && args.len() > 10 { Some(parse_int(args[10])?) } else { None };
    let mut n = 0;
    for z in lo.z..=hi.z {
        for y in lo.y..=hi.y {
            for x in lo.x..=hi.x {
                let p = BlockPos::new(x, y, z);
                let inside = x != lo.x && x != hi.x && y != lo.y && y != hi.y && z != lo.z && z != hi.z;
                match mode {
                    Some("outline" | "hollow") if inside => {
                        if mode == Some("hollow") && r.c.world.set_block_state(p, mc_data::StateId::AIR).is_some() {
                            r.out.edits.push(p);
                        }
                        continue;
                    }
                    Some("destroy") => {
                        if r.c.world.set_block_state(p, mc_data::StateId::AIR).is_some() {
                            r.out.edits.push(p);
                        }
                    }
                    Some("keep") if !r.c.world.is_air(p) => continue,
                    Some("replace") => {
                        let s = r.c.world.state_pos(p);
                        if filter.is_some_and(|f| s.block().id != f) || filter_meta.is_some_and(|m| s.meta() as i32 != m) {
                            continue;
                        }
                    }
                    _ => {}
                }
                if let Some(c) = r.c.world.chunks.get_mut(&(x >> 4, z >> 4)) {
                    if c.tile_entities.remove(&p).is_some() {
                        r.c.world.set_block_state(p, Block::by_name("minecraft:barrier").map_or(mc_data::StateId::AIR, |b| b.default_state));
                    }
                }
                if r.c.world.set_block_state(p, state).is_some() {
                    r.out.edits.push(p);
                    n += 1;
                }
            }
        }
    }
    if n <= 0 {
        return err("commands.fill.failed", vec![]);
    }
    r.notify("commands.fill.success", vec![n.to_string()]);
    Ok(())
}

fn test_for_block(r: &mut Run, args: &[&str]) -> Res {
    if args.len() < 4 {
        return Err(Error::Usage("commands.testforblock.usage"));
    }
    let pos = r.parse_block_pos(args, 0, false)?;
    // Block.getBlockFromName: registry name, then numeric id.
    let block = Block::by_name(&resource_location(args[3]))
        .or_else(|| java_int(args[3]).and_then(|i| u16::try_from(i).ok()).and_then(Block::by_id))
        .ok_or_else(|| Error::Cmd(msg("commands.setblock.notFound", vec![args[3].into()])))?;
    let i = if args.len() >= 5 { parse_int_range(args[4], -1, 15)? } else { -1 };
    if !r.c.world.is_block_loaded(pos) {
        return err("commands.testforblock.outOfWorld", vec![]);
    }
    let s = r.c.world.state_pos(pos);
    let (x, y, z) = (pos.x.to_string(), pos.y.to_string(), pos.z.to_string());
    if s.block().id != block.id {
        let (have, want) = (r.block_name(s.block()), r.block_name(block));
        return err("commands.testforblock.failed.tile", vec![x, y, z, have, want]);
    }
    if i > -1 && s.meta() as i32 != i {
        return err("commands.testforblock.failed.data", vec![x, y, z, s.meta().to_string(), i.to_string()]);
    }
    r.notify("commands.testforblock.success", vec![x, y, z]);
    Ok(())
}

fn game_rule(r: &mut Run, args: &[&str]) -> Res {
    let s = args.first().copied().unwrap_or("");
    let s1 = if args.len() > 1 { build_string(args, 1) } else { String::new() };
    match args.len() {
        0 => {
            let names: Vec<String> = r.c.info.rules.names().into_iter().map(String::from).collect();
            r.send_raw(join_nice(&names));
        }
        1 => {
            if !r.c.info.rules.has(s) {
                return err("commands.gamerule.norule", vec![s.into()]);
            }
            let v = r.c.info.rules.get_string(s);
            r.send_raw(format!("{s} = {v}"));
        }
        _ => {
            if r.c.info.rules.is_boolean(s) && s1 != "true" && s1 != "false" {
                return err("commands.generic.boolean.invalid", vec![s1]);
            }
            r.c.info.rules.set(s, &s1);
            r.notify("commands.gamerule.success", vec![]);
        }
    }
    Ok(())
}

/// `CommandBase.joinNiceString`
fn join_nice(v: &[String]) -> String {
    let mut out = String::new();
    for (i, s) in v.iter().enumerate() {
        if i > 0 {
            out.push_str(if i == v.len() - 1 { " and " } else { ", " });
        }
        out.push_str(s);
    }
    out
}

fn seed(r: &mut Run, _args: &[&str]) -> Res {
    let s = r.c.info.seed.to_string();
    r.send(msg("commands.seed.success", vec![s]));
    Ok(())
}

fn set_world_spawn(r: &mut Run, args: &[&str]) -> Res {
    let pos = if args.is_empty() {
        r.sender_pos()
    } else {
        if args.len() != 3 {
            return Err(Error::Usage("commands.setworldspawn.usage"));
        }
        r.parse_block_pos(args, 0, true)?
    };
    r.c.info.spawn = pos;
    r.notify("commands.setworldspawn.success", vec![pos.x.to_string(), pos.y.to_string(), pos.z.to_string()]);
    Ok(())
}

fn spawn_point(r: &mut Run, args: &[&str]) -> Res {
    if args.len() > 1 && args.len() < 4 {
        return Err(Error::Usage("commands.spawnpoint.usage"));
    }
    if let Some(a) = args.first() {
        r.player(a)?;
    }
    let pos = if args.len() > 3 { r.parse_block_pos(args, 1, true)? } else { r.sender_pos() };
    r.c.player.spawn_point = Some(pos);
    let name = r.c.name.to_string();
    r.notify("commands.spawnpoint.success", vec![name, pos.x.to_string(), pos.y.to_string(), pos.z.to_string()]);
    Ok(())
}

fn say(r: &mut Run, args: &[&str]) -> Res {
    if args.is_empty() || args[0].is_empty() {
        return Err(Error::Usage("commands.say.usage"));
    }
    let text = chat_args(r, args, 0)?;
    let name = r.c.name.to_string();
    r.send(msg("chat.type.announcement", vec![name, text]));
    Ok(())
}

fn emote(r: &mut Run, args: &[&str]) -> Res {
    if args.is_empty() {
        return Err(Error::Usage("commands.me.usage"));
    }
    // A player sender's words are not selector-expanded.
    let text = build_string(args, 0);
    let name = r.c.name.to_string();
    r.send(msg("chat.type.emote", vec![name, text]));
    Ok(())
}

/// `getChatComponentFromNthArg(sender, args, i, true)`: selectors expand to entity names.
fn chat_args(r: &Run, args: &[&str], start: usize) -> Result<String, Error> {
    let mut out = Vec::new();
    for a in &args[start..] {
        if selector(a).is_some() {
            if r.matches_player(a) {
                out.push(r.c.name.to_string());
            } else {
                return err("commands.generic.player.notFound", vec![]);
            }
        } else {
            out.push(a.to_string());
        }
    }
    Ok(out.join(" "))
}

fn tell(r: &mut Run, args: &[&str]) -> Res {
    if args.len() < 2 {
        return Err(Error::Usage("commands.message.usage"));
    }
    r.player(args[0])?;
    // The only player is the sender.
    err("commands.message.sameTarget", vec![])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_number_formatting() {
        assert_eq!(java_double(64.0), "64.0");
        assert_eq!(java_double(10.5), "10.5");
        assert_eq!(java_double(-3.0e7), "-3.0E7");
        assert_eq!(java_double(1.0e-4), "1.0E-4");
        assert_eq!(java_double(0.1), "0.1");
    }

    #[test]
    fn coordinates_center_and_relative() {
        assert_eq!(parse_coord(5.0, "10", -30_000_000, 30_000_000, true).unwrap().0, 10.5);
        assert_eq!(parse_coord(5.0, "10.0", -30_000_000, 30_000_000, true).unwrap().0, 10.0);
        assert_eq!(parse_coord(5.0, "~2", -30_000_000, 30_000_000, true).unwrap().0, 7.0);
        assert_eq!(parse_coord(5.0, "~", 0, 0, false).unwrap(), (5.0, 0.0, true));
        assert!(parse_coord(0.0, "NaN", 0, 0, false).is_err());
        assert!(parse_coord(0.0, "Infinity", 0, 0, false).is_err());
    }

    #[test]
    fn commands_change_real_state() {
        let Some(jar) = crate::find_jar() else { return };
        let mut assets = mc_assets::AssetSource::new();
        assets.push(&jar).unwrap();
        let mut ui = Ui::new(&assets, mc_gui::settings::Settings::default());
        let items = Catalogue::metadata();
        let mut world = World::default();
        for (x, z) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            world.insert(mc_world::chunk::Chunk::new(x, z));
        }
        let mut player = Player::new(Vec3::new(8., 2., 8.));
        let mut info = WorldInfo::new(42, BlockPos::new(0, 64, 0));
        let mut run = |s: &str, player: &mut Player, world: &mut World, ui: &mut Ui| {
            let mut c = Ctx { player, world, ui, items: &items, info: &mut info, name: "Player" };
            execute(s, &mut c).messages
        };
        assert_eq!(run("/give Player stone 64 2", &mut player, &mut world, &mut ui), ["Given [Polished Granite] * 64 to Player"]);
        assert_eq!(run("/give Player stone 65", &mut player, &mut world, &mut ui), ["\u{a7}cThe number you have entered (65) is too big, it must be at most 64"]);
        assert_eq!(run("/give Player fake_item", &mut player, &mut world, &mut ui), ["\u{a7}cThere is no such item with name minecraft:fake_item"]);
        assert_eq!(run("/gamemode 0", &mut player, &mut world, &mut ui), ["Your game mode has been updated", "Set own game mode to Survival Mode"]);
        assert_eq!(player.game_type, GameType::Survival);
        assert_eq!(run("/tp 3 4 5", &mut player, &mut world, &mut ui), ["Teleported Player to 3.5, 4.0, 5.5"]);
        assert_eq!(player.pos, Vec3::new(3.5, 4., 5.5));
        assert_eq!(run("/fill 1 1 1 3 1 3 planks 2", &mut player, &mut world, &mut ui), ["9 blocks filled"]);
        assert_eq!(world.get(3, 1, 3), (5, 2));
        assert_eq!(run("/fill 30 2 30 33 2 33 stone", &mut player, &mut world, &mut ui), ["\u{a7}cCannot place blocks outside of the world"]);
        assert_eq!(run("/setblock 1 1 1 planks 2 keep", &mut player, &mut world, &mut ui), ["\u{a7}cThe block couldn't be placed"]);
        assert_eq!(run("/time set day", &mut player, &mut world, &mut ui), ["Set the time to 1000"]);
        assert_eq!(run("/time query daytime", &mut player, &mut world, &mut ui), ["Time is 1000"]);
        assert_eq!(run("/foo", &mut player, &mut world, &mut ui), ["\u{a7}cUnknown command. Try /help for a list of commands"]);
        assert_eq!(run("/gamemode", &mut player, &mut world, &mut ui), ["\u{a7}cUsage: /gamemode <mode> [player]"]);
        assert_eq!(run("/kill Bob", &mut player, &mut world, &mut ui), ["\u{a7}cThe entity UUID provided is in an invalid format"]);
        assert_eq!(run("/gamerule doDaylightCycle", &mut player, &mut world, &mut ui), ["doDaylightCycle = true"]);
        assert_eq!(run("/seed", &mut player, &mut world, &mut ui), ["Seed: 42"]);
        assert_eq!(run("/clear", &mut player, &mut world, &mut ui), ["Cleared the inventory of Player, removing 64 items"]);
        assert_eq!(run("/kill", &mut player, &mut world, &mut ui), ["Player fell out of the world", "Killed Player"]);
        assert!(player.dead);
    }
}
