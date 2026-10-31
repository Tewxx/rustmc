//! Client glue: the chat, options, screen and connection stubs whose state the Rust client owns, plus the queue of
//! effects a mod asks of it. Plain logic over mirrored stub state lives in `rustmc.forge.ClientGlue`.
use super::minecraft;
use crate::{runtime as rt, stubs::{register, Call, Ret}};
use jni::{errors::Result, objects::{GlobalRef, JObject, JString, JValue}, JNIEnv};
use std::sync::{atomic::{AtomicBool, Ordering}, Mutex};

const GLUE: &str = "rustmc/forge/ClientGlue";
const COMPONENT: &str = "Lnet/minecraft/util/IChatComponent;";

/// An effect a mod asked the Rust client to perform; drained by [`take_client_actions`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientAction {
    /// `GuiNewChat.printChatMessage`: add to the local chat. `json` is a serialized `IChatComponent`, the same shape
    /// as Live's `local_chat`; a non-zero `id` replaces the line with that id.
    Chat { json: String, id: i32 },
    /// `GuiNewChat.clearChatMessages`.
    ClearChat,
    /// `GuiNewChat.deleteChatLine`.
    DeleteChatLine(i32),
    /// `GuiNewChat.refreshChat`: re-wrap the chat lines for the current width.
    RefreshChat,
    /// `GuiNewChat.scroll`.
    ScrollChat(i32),
    /// `GuiNewChat.addToSentMessages`: the up-arrow recall history.
    AddSentMessage(String),
    /// `Minecraft.func_71400_g`: quit.
    Shutdown,
    /// `Minecraft.func_71352_k`: toggle fullscreen.
    ToggleFullscreen,
    /// `Minecraft.func_71370_a`: resize the display.
    Resize { width: i32, height: i32 },
    /// `GameSettings.func_74303_b`: persist the options.
    SaveOptions,
    /// `ScreenShotHelper.saveScreenshot`: write `path` from the frame being drawn.
    Screenshot { path: String },
    /// `NetworkManager.createNetworkManagerAndConnect`.
    Connect { host: String, port: u16 },
    /// `NetworkManager.closeChannel`: drop the connection, showing `reason` (component JSON) if the mod gave one.
    Disconnect { reason: Option<String> },
    /// `OldServerPinger.ping`: fetch a server's status line (address is `ServerData.serverIP`).
    Ping { address: String, id: u64 },
    /// `PlayerControllerMP.windowClick`: the click is sent by the client, which owns the open container.
    WindowClick { window_id: i32, slot: i32, button: i32, mode: i32 },
    /// `PlayerControllerMP.resetBlockRemoving`: stop breaking the current block.
    StopBreaking,
    /// `RenderGlobal.loadRenderers`: rebuild every loaded section's mesh (F3+A).
    ReloadRenderers,
    /// `Minecraft.refreshResources` / `scheduleResourcesRefresh`: reload the resource pack stack (F3+T).
    ReloadResources,
}

static ACTIONS: Mutex<Vec<ClientAction>> = Mutex::new(Vec::new());
static CHAT_OPEN: AtomicBool = AtomicBool::new(false);
/// Client commands mods registered through `ClientCommandHandler.registerCommand`, by every name they answer to.
static COMMANDS: Mutex<Vec<(String, GlobalRef)>> = Mutex::new(Vec::new());

fn push(action: ClientAction) { ACTIONS.lock().unwrap().push(action); }

/// Effects mods asked of the client since the last call; the client drains this every frame.
pub fn take_client_actions() -> Vec<ClientAction> { std::mem::take(&mut *ACTIONS.lock().unwrap()) }

/// The client's chat state, read by `GuiNewChat.getChatOpen` (vanilla's `currentScreen instanceof GuiChat`).
pub fn set_chat_open(open: bool) { CHAT_OPEN.store(open, Ordering::Relaxed); }
pub fn is_chat_open() -> bool { CHAT_OPEN.load(Ordering::Relaxed) }

// `Minecraft`'s window methods ask for the same effects.
pub(crate) fn push_resize(width: i32, height: i32) { push(ClientAction::Resize { width, height }); }
pub(crate) fn push_reload_resources() { push(ClientAction::ReloadResources); }
pub(crate) fn push_shutdown() { push(ClientAction::Shutdown); }
pub(crate) fn push_toggle_fullscreen() { push(ClientAction::ToggleFullscreen); }

static NEXT_PING: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// `ServerData` objects waiting for a ping result, by [`ClientAction::Ping`] id.
static PINGED: Mutex<Vec<(u64, GlobalRef)>> = Mutex::new(Vec::new());

/// The result of a mod-requested ping, written into its `ServerData` as `OldServerPinger` would.
pub struct PingResult<'a> { pub motd: &'a str, pub population: &'a str, pub ping: i64, pub version: i32, pub game_version: &'a str, pub player_list: Option<&'a str>, pub icon: Option<&'a str> }

pub fn finish_ping(env: &mut JNIEnv<'_>, id: u64, r: &PingResult<'_>) -> Result<()> {
    let server = { let mut p = PINGED.lock().unwrap(); match p.iter().position(|(i, _)| *i == id) { Some(i) => p.remove(i).1, None => return Ok(()) } };
    let server = env.new_local_ref(server.as_obj())?;
    const S: &str = "Ljava/lang/String;";
    for (field, value) in [("field_78843_d", Some(r.motd)), ("field_78846_c", Some(r.population)), ("field_82822_g", Some(r.game_version)), ("field_147412_i", r.player_list), ("field_147411_m", r.icon)] {
        let v = match value { Some(v) => JObject::from(env.new_string(v)?), None => JObject::null() };
        env.set_field(&server, field, S, JValue::Object(&v))?;
    }
    env.set_field(&server, "field_78844_e", "J", JValue::Long(r.ping))?;
    env.set_field(&server, "field_82821_f", "I", JValue::Int(r.version))
}

/// Mod-registered client commands as (name, command object), for the client's `/` dispatcher.
pub fn client_commands() -> Vec<(String, GlobalRef)> { COMMANDS.lock().unwrap().clone() }

/// Runs a mod-registered client command the way `ClientCommandHandler` would: `name` plus its arguments, from the
/// player. `false` when no mod owns the name (the client then falls back to its own commands).
pub fn execute_client_command(env: &mut JNIEnv<'_>, name: &str, args: &[String]) -> Result<bool> {
    let Some((_, cmd)) = COMMANDS.lock().unwrap().iter().find(|(n, _)| n == name).cloned() else { return Ok(false) };
    let cmd = env.new_local_ref(cmd.as_obj())?;
    let mc = minecraft::minecraft(env)?;
    let sender = env.get_field(&mc, "field_71439_g", "Lnet/minecraft/entity/player/EntityPlayer;")?.l()?;
    let elems: Vec<String> = std::iter::once(name.to_owned()).chain(args.iter().cloned()).collect();
    let array = env.new_object_array(elems.len() as i32, "java/lang/String", JObject::null())?;
    for (i, arg) in elems.iter().enumerate() {
        let s = env.new_string(arg)?;
        env.set_object_array_element(&array, i as i32, s)?;
    }
    env.call_method(&cmd, "func_71515_b", &format!("(Lnet/minecraft/command/ICommandSender;[Ljava/lang/String;)Z"),
        &[JValue::Object(&sender), JValue::Object(&array)]).or_else(|e| match e {
        // ClientCommandHandler catches the command's own failure (usage/CommandException) and still consumes it.
        jni::errors::Error::JavaException => { let _ = env.exception_describe(); env.exception_clear().map(|_| jni::objects::JValueOwned::Void) }
        e => Err(e),
    })?;
    Ok(true)
}

pub(crate) fn register_all() {
    render_global();
    chat();
    screens();
    controller();
    network();
    settings();
    font();
    java_effects();
}

/// The Java half queues its effects here (`ClientGlue.action`).
fn java_effects() {
    register(&format!("{GLUE}.action"), |c| {
        let kind = c.string(0)?.unwrap_or_default();
        let values = c.obj(1)?;
        match kind.as_str() {
            "screenshot" => match array_string(c.env, &values, 0)? {
                Some(path) => push(ClientAction::Screenshot { path }),
                None => {}
            },
            _ => {}
        }
        Ok(Ret::Null)
    });
}

fn array_string(env: &mut JNIEnv<'_>, array: &JObject<'_>, index: i32) -> Result<Option<String>> {
    // SAFETY: callers pass an Object[] local reference; the wrapper does not own or free it.
    let array = unsafe { jni::objects::JObjectArray::from_raw(array.as_raw()) };
    let value = env.get_object_array_element(&array, index)?;
    if value.is_null() { return Ok(None); }
    let value = JString::from(value);
    let text: String = env.get_string(&value)?.into();
    Ok(Some(text))
}

/// `GuiNewChat`: the chat is the client's, so printing queues an action and the sizes come from `GameSettings`.
fn chat() {
    const CHAT: &str = "net/minecraft/client/gui/GuiNewChat.";
    register(&format!("{CHAT}func_146241_e"), |_| Ok(Ret::Bool(is_chat_open())));
    register(&format!("{CHAT}func_146244_h"), |c| Ok(Ret::Float(setting(c.env, "field_96691_E", 1.0)?)));
    register(&format!("{CHAT}func_146228_f"), |c| Ok(Ret::Int((setting(c.env, "field_96692_F", 1.0)? * 280.0 + 40.0).floor() as i32)));
    register(&format!("{CHAT}func_146246_g"), |c| Ok(Ret::Int(chat_height(c.env)?)));
    register(&format!("{CHAT}func_146232_i"), |c| Ok(Ret::Int(chat_height(c.env)? / 9)));
    register(&format!("{CHAT}func_146227_a"), |c| print(c, 0));
    register(&format!("{CHAT}func_146234_a"), |c| { let id = c.i32(1)?; print(c, id) });
    register(&format!("{CHAT}func_146231_a"), |_| { push(ClientAction::ClearChat); Ok(Ret::Null) });
    register(&format!("{CHAT}func_146245_b"), |_| { push(ClientAction::RefreshChat); Ok(Ret::Null) });
    register(&format!("{CHAT}func_146242_c"), |c| { push(ClientAction::DeleteChatLine(c.i32(0)?)); Ok(Ret::Null) });
    register(&format!("{CHAT}func_146229_b"), |c| { push(ClientAction::ScrollChat(c.i32(0)?)); Ok(Ret::Null) });
    register(&format!("{CHAT}func_146239_a"), |c| {
        match c.string(0)? { Some(m) => push(ClientAction::AddSentMessage(m)), None => {} }
        Ok(Ret::Null)
    });
    // GuiIngame.getChatGUI: the one GuiNewChat, allocated on first use.
    register("net/minecraft/client/gui/GuiIngame.func_146158_b", |c| {
        static CHAT_GUI: Mutex<Option<GlobalRef>> = Mutex::new(None);
        if let Some(g) = CHAT_GUI.lock().unwrap().clone() { return Ok(Ret::Obj(g)); }
        let gui = rt::alloc(c.env, "net/minecraft/client/gui/GuiNewChat")?;
        let g = c.env.new_global_ref(gui)?;
        *CHAT_GUI.lock().unwrap() = Some(g.clone());
        Ok(Ret::Obj(g))
    });
    // ChatLine: the component and ids are the object's own fields.
    register("net/minecraft/client/gui/ChatLine.<init>", |c| {
        let counter = c.i32(0)?;
        let line = c.obj(1)?;
        let id = c.i32(2)?;
        c.env.set_field(&c.this, "field_74543_a", "I", JValue::Int(counter))?;
        c.env.set_field(&c.this, "field_74541_b", COMPONENT, JValue::Object(&line))?;
        c.env.set_field(&c.this, "field_74542_c", "I", JValue::Int(id))?;
        Ok(Ret::Null)
    });
    register("net/minecraft/client/gui/ChatLine.func_151461_a", |c| {
        let line = c.env.get_field(&c.this, "field_74541_b", COMPONENT)?.l()?;
        Ok(if line.is_null() { Ret::Null } else { Ret::Obj(c.env.new_global_ref(line)?) })
    });
}

/// Vanilla `GuiNewChat.getChatHeight`: the focused height while typing, the small one otherwise.
fn chat_height(env: &mut JNIEnv<'_>) -> Result<i32> {
    let scale = if is_chat_open() { setting(env, "field_96694_H", 1.0)? } else { setting(env, "field_96693_G", 0.5)? };
    Ok((scale * 160.0 + 20.0).floor() as i32)
}

/// A chat box `GameSettings` float; an unmirrored (zero) value means vanilla's default.
fn setting(env: &mut JNIEnv<'_>, field: &str, default: f32) -> Result<f32> {
    let mc = minecraft::minecraft(env)?;
    let gs = env.get_field(&mc, "field_71474_y", "Lnet/minecraft/client/settings/GameSettings;")?.l()?;
    if gs.is_null() { return Ok(default); }
    let value = env.get_field(&gs, field, "F")?.f()?;
    Ok(if value == 0.0 { default } else { value })
}

fn print(c: &mut Call<'_, '_>, id: i32) -> Result<Ret> {
    let comp = c.obj(0)?;
    if let Some(json) = component_json(c.env, &comp)? { push(ClientAction::Chat { json, id }); }
    Ok(Ret::Null)
}

/// `IChatComponent.Serializer.componentToJson`; `None` for a null component (vanilla prints nothing).
fn component_json(env: &mut JNIEnv<'_>, comp: &JObject<'_>) -> Result<Option<String>> {
    if comp.is_null() { return Ok(None); }
    let sig = format!("({COMPONENT})Ljava/lang/String;");
    let s = rt::call_static(env, "net/minecraft/util/IChatComponent$Serializer", "func_150696_a", &sig, &[JValue::Object(comp)])?.l()?;
    if s.is_null() { return Ok(None); }
    let s = JString::from(s);
    let text: String = env.get_string(&s)?.into();
    Ok(Some(text))
}

/// Screen constructors: a stub constructor only keeps its arguments, so the parent/options fields are set here.
fn screens() {
    const SCREEN: &str = "Lnet/minecraft/client/gui/GuiScreen;";
    const SETTINGS: &str = "Lnet/minecraft/client/settings/GameSettings;";
    for (key, parent_field, settings_field) in [
        ("net/minecraft/client/gui/GuiMultiplayer.<init>", "field_146798_g", None),
        ("net/minecraft/client/gui/GuiSelectWorld.<init>", "field_146632_a", None),
        ("net/minecraft/client/gui/GuiOptions.<init>", "field_146441_g", Some("field_146443_h")),
        ("net/minecraft/client/gui/GuiVideoSettings.<init>", "field_146498_f", Some("field_146499_g")),
    ] {
        register(key, move |c| {
            let parent = c.obj(0)?;
            c.env.set_field(&c.this, parent_field, SCREEN, JValue::Object(&parent))?;
            if let Some(field) = settings_field {
                match c.obj(1) {
                    Ok(settings) if !settings.is_null() => { c.env.set_field(&c.this, field, SETTINGS, JValue::Object(&settings))?; }
                    _ => { let settings = rt::alloc(c.env, "net/minecraft/client/settings/GameSettings")?;
                           c.env.set_field(&c.this, field, SETTINGS, JValue::Object(&settings))?; }
                }
            }
            Ok(Ret::Null)
        });
    }
    // Forge's mod list keeps its parent in Forge fields this side has no names for; the screen itself is the client's.
    register("net/minecraftforge/fml/client/GuiModList.<init>", |_| Ok(Ret::Null));
}

/// `PlayerControllerMP`: the client drives mining and containers, so it gets told instead of the other way round.
fn render_global() {
    register("net/minecraft/client/renderer/RenderGlobal.func_72712_a", |_| { push(ClientAction::ReloadRenderers); Ok(Ret::Null) });
}

fn controller() {
    const PC: &str = "net/minecraft/client/multiplayer/PlayerControllerMP.";
    register(&format!("{PC}func_181040_m"), |c| {
        Ok(Ret::Bool(c.env.get_field(&c.this, "field_78778_j", "Z")?.z()?))
    });
    register(&format!("{PC}func_78747_a"), |c| {
        let game_type = c.env.get_field(&c.this, "field_78779_k", "Lnet/minecraft/world/WorldSettings$GameType;")?.l()?;
        if game_type.is_null() { return Ok(Ret::Bool(false)); }
        let spectator = rt::static_field(c.env, "net/minecraft/world/WorldSettings$GameType", "SPECTATOR",
            "Lnet/minecraft/world/WorldSettings$GameType;")?.l()?;
        Ok(Ret::Bool(!spectator.is_null() && c.env.is_same_object(&game_type, &spectator)?))
    });
    register(&format!("{PC}func_78767_c"), |c| {
        c.env.set_field(&c.this, "field_78778_j", "Z", JValue::Bool(0))?;
        push(ClientAction::StopBreaking);
        Ok(Ret::Null)
    });
    register(&format!("{PC}func_78753_a"), |c| {
        let (window_id, slot, button, mode) = (c.i32(0)?, c.i32(1)?, c.i32(2)?, c.i32(3)?);
        push(ClientAction::WindowClick { window_id, slot, button, mode });
        // The Rust client owns the container and the reply; nothing has moved yet on this side.
        Ok(Ret::Null)
    });
    // NetHandlerPlayClient.getNetworkManager: the field Live's socket mirror lives in.
    register("net/minecraft/client/network/NetHandlerPlayClient.func_147298_b", |c| {
        let manager = c.env.get_field(&c.this, "field_147302_e", "Lnet/minecraft/network/NetworkManager;")?.l()?;
        Ok(if manager.is_null() { Ret::Null } else { Ret::Obj(c.env.new_global_ref(manager)?) })
    });
}

/// `NetworkManager` and `OldServerPinger`: connections are the client's, so these only queue work.
fn network() {
    const NM: &str = "net/minecraft/network/NetworkManager.";
    register(&format!("{NM}func_150718_a"), |c| {
        let reason = c.obj(0)?;
        push(ClientAction::Disconnect { reason: component_json(c.env, &reason)? });
        Ok(Ret::Null)
    });
    register(&format!("{NM}func_150719_a"), |c| {
        let handler = c.obj(0)?;
        c.env.set_field(&c.this, "field_150744_m", "Lnet/minecraft/network/INetHandler;", JValue::Object(&handler))?;
        Ok(Ret::Null)
    });
    register(&format!("{NM}func_181124_a"), |c| {
        let address = c.obj(0)?;
        let port = c.i32(1)?;
        let host = if address.is_null() { String::new() } else {
            let s = c.env.call_method(&address, "getHostAddress", "()Ljava/lang/String;", &[])?.l()?;
            if s.is_null() { String::new() } else { c.env.get_string(&JString::from(s))?.into() }
        };
        push(ClientAction::Connect { host, port: port as u16 });
        let manager = rt::alloc(c.env, "net/minecraft/network/NetworkManager")?;
        Ok(Ret::Obj(c.env.new_global_ref(manager)?))
    });
    const PINGER: &str = "net/minecraft/client/network/OldServerPinger.";
    register(&format!("{PINGER}<init>"), |_| Ok(Ret::Null));
    register(&format!("{PINGER}func_147223_a"), |_| Ok(Ret::Null)); // pingPendingNetworks: nothing is pending
    register(&format!("{PINGER}func_147224_a"), |c| {
        let server = c.obj(0)?;
        if !server.is_null() {
            let ip = c.env.get_field(&server, "field_78845_b", "Ljava/lang/String;")?.l()?;
            if !ip.is_null() {
                let address: String = c.env.get_string(&JString::from(ip))?.into();
                let id = NEXT_PING.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                PINGED.lock().unwrap().push((id, c.env.new_global_ref(&server)?));
                // vanilla marks the entry pinging before the request goes out
                let text = c.env.new_string("Pinging...")?;
                c.env.set_field(&server, "field_78843_d", "Ljava/lang/String;", JValue::Object(&text))?;
                c.env.set_field(&server, "field_78844_e", "J", JValue::Long(-1))?;
                push(ClientAction::Ping { address, id });
            }
        }
        Ok(Ret::Null)
    });
}

/// `GameSettings`, `Minecraft`'s option-adjacent bits and `ScreenShotHelper`.
fn settings() {
    register("net/minecraft/client/settings/GameSettings.func_74298_c", |c| {
        let key = c.i32(0)?;
        let s = rt::call_static(c.env, GLUE, "keyDisplay", "(I)Ljava/lang/String;", &[JValue::Int(key)])?.l()?;
        Ok(Ret::Str(c.env.get_string(&JString::from(s))?.into()))
    });
    register("net/minecraft/client/settings/GameSettings.func_74303_b", |_| {
        push(ClientAction::SaveOptions);
        Ok(Ret::Null)
    });
    register("net/minecraft/util/ScreenShotHelper.func_148259_a", |c| {
        let dir = c.obj(0)?;
        let name = c.obj(1)?;
        let (width, height) = (c.i32(2)?, c.i32(3)?);
        let result = rt::call_static(c.env, GLUE, "screenshot",
            "(Ljava/io/File;Ljava/lang/String;II)Lnet/minecraft/util/IChatComponent;",
            &[JValue::Object(&dir), JValue::Object(&name), JValue::Int(width), JValue::Int(height)])?.l()?;
        Ok(if result.is_null() { Ret::Null } else { Ret::Obj(c.env.new_global_ref(result)?) })
    });
    // `public static final ClientCommandHandler instance = new ClientCommandHandler()`: the stub's <clinit> only
    // dispatches here, so allocate the singleton (JNI may write a static final field).
    register("net/minecraftforge/client/ClientCommandHandler.<clinit>", |c| {
        const CLASS: &str = "net/minecraftforge/client/ClientCommandHandler";
        let handler = rt::alloc(c.env, CLASS)?;
        let class = rt::class(c.env, CLASS)?;
        c.env.set_static_field(&class, (&class, "instance", "Lnet/minecraftforge/client/ClientCommandHandler;"), JValue::Object(&handler))?;
        Ok(Ret::Null)
    });
    // ClientCommandHandler.registerCommand: keep the command for the client's `/` dispatcher and pass it back.
    // Called through CommandHandler.registerCommand too (an invokevirtual on the superclass).
    for key in ["net/minecraftforge/client/ClientCommandHandler.func_71560_a", "net/minecraft/command/CommandHandler.func_71560_a"] {
    register(key, |c| {
        let command = c.obj(0)?;
        if !command.is_null() {
            let name = c.env.call_method(&command, "func_71517_b", "()Ljava/lang/String;", &[])?.l()?;
            if !name.is_null() {
                let name: String = c.env.get_string(&JString::from(name))?.into();
                if !name.is_empty() { COMMANDS.lock().unwrap().push((name, c.env.new_global_ref(&command)?)); }
            }
        }
        Ok(if command.is_null() { Ret::Null } else { Ret::Obj(c.env.new_global_ref(command)?) })
    });
    }
}

/// `FontRenderer.getColorCode`: vanilla's formatting colour table, built the way the constructor builds it.
fn font() {
    register("net/minecraft/client/gui/FontRenderer.func_175064_b", |c| {
        Ok(Ret::Int(color_code(c.char(0)?)))
    });
}

/// `FontRenderer.colorCode[index]` for one of `0123456789abcdef` (`#FFFFFF` masks, so 0xRRGGBB).
fn color_code(character: char) -> i32 {
    let index = match "0123456789abcdef".find(character) { Some(i) => i as i32, None => return 0 };
    let base = ((index >> 3) & 1) * 85;
    let mut r = ((index >> 2) & 1) * 170 + base;
    let mut g = ((index >> 1) & 1) * 170 + base;
    let mut b = (index & 1) * 170 + base;
    if index == 6 { r += 85; }
    if index >= 16 { r /= 4; g /= 4; b /= 4; }
    (r << 16) | (g << 8) | b
}

#[cfg(test)]
mod tests {
    use super::color_code;

    #[test]
    fn vanilla_color_codes() {
        assert_eq!(color_code('0'), 0x000000);
        assert_eq!(color_code('1'), 0x0000AA);
        assert_eq!(color_code('c'), 0xFF5555);
        assert_eq!(color_code('f'), 0xFFFFFF);
        assert_eq!(color_code('z'), 0);
    }
}
