//! What the client calls at the points vanilla Forge fires events: ticks, screens, input.
//! Every function runs on the thread attached to the embedded JVM (`runtime::with_env`).
use crate::impls::{minecraft, minecraft_display, minecraft_gui_size, set_event, MouseEvent};
use crate::runtime as rt;
use jni::{errors::Result, objects::{GlobalRef, JObject, JValue}, JNIEnv};
use std::sync::Mutex;

const EVENT: &str = "Lnet/minecraftforge/fml/common/eventhandler/Event;";
const PHASE: &str = "net/minecraftforge/fml/common/gameevent/TickEvent$Phase";
const SCREEN: &str = "Lnet/minecraft/client/gui/GuiScreen;";
const MC: &str = "Lnet/minecraft/client/Minecraft;";

/// vanilla `handleMouseInput` divides pixel positions by `mc.displayWidth/Height`, so they must be the live window size.
fn sync_display(env: &mut JNIEnv<'_>) -> Result<()> {
    let mc = minecraft(env)?;
    let d = minecraft_display();
    env.set_field(&mc, "field_71443_c", "I", JValue::Int(d[2]))?;
    env.set_field(&mc, "field_71440_d", "I", JValue::Int(d[3]))
}

/// Posts `event` on the shared bus; true if a listener cancelled it.
pub fn post(env: &mut JNIEnv<'_>, event: &JObject<'_>) -> Result<bool> {
    rt::call_static(env, "rustmc/forge/Bus", "post", &format!("({EVENT})Z"), &[JValue::Object(event)])?.z()
}

fn phase<'l>(env: &mut JNIEnv<'l>, start: bool) -> Result<JObject<'l>> {
    env.get_static_field(PHASE, if start { "START" } else { "END" }, &format!("L{PHASE};"))?.l()
}

/// `TickEvent.ClientTickEvent` START or END, once per game tick.
pub fn client_tick(env: &mut JNIEnv<'_>, start: bool) -> Result<()> {
    let p = phase(env, start)?;
    let e = rt::new_object(env, "net/minecraftforge/fml/common/gameevent/TickEvent$ClientTickEvent", &format!("(L{PHASE};)V"), &[JValue::Object(&p)])?;
    post(env, &e)?;
    if let Some(screen) = current_screen(env)? { if !start { env.call_method(&screen, "func_73876_c", "()V", &[])?; } }
    Ok(())
}

/// `TickEvent.RenderTickEvent` START or END, once per frame.
pub fn render_tick(env: &mut JNIEnv<'_>, start: bool, partial_ticks: f32) -> Result<()> {
    let p = phase(env, start)?;
    let e = rt::new_object(env, "net/minecraftforge/fml/common/gameevent/TickEvent$RenderTickEvent", &format!("(L{PHASE};F)V"), &[JValue::Object(&p), JValue::Float(partial_ticks)])?;
    post(env, &e).map(drop)
}

fn the_player<'l>(env: &mut JNIEnv<'l>) -> Result<Option<JObject<'l>>> {
    let mc = minecraft(env)?;
    let p = env.get_field(&mc, "field_71439_g", "Lnet/minecraft/client/entity/EntityPlayerSP;")?.l()?;
    Ok((!p.is_null()).then_some(p))
}

/// `TickEvent.PlayerTickEvent` around `EntityPlayer.onUpdate`, for the local player.
pub fn player_tick(env: &mut JNIEnv<'_>, start: bool) -> Result<()> {
    let Some(player) = the_player(env)? else { return Ok(()) };
    let p = phase(env, start)?;
    let e = rt::new_object(env, "net/minecraftforge/fml/common/gameevent/TickEvent$PlayerTickEvent",
        &format!("(L{PHASE};Lnet/minecraft/entity/player/EntityPlayer;)V"), &[JValue::Object(&p), JValue::Object(&player)])?;
    post(env, &e).map(drop)
}

/// `ForgeHooks.onLivingUpdate` for the local player; true if a listener cancelled the update.
pub fn living_update(env: &mut JNIEnv<'_>) -> Result<bool> {
    let Some(player) = the_player(env)? else { return Ok(false) };
    let e = rt::new_object(env, "net/minecraftforge/event/entity/living/LivingEvent$LivingUpdateEvent",
        "(Lnet/minecraft/entity/EntityLivingBase;)V", &[JValue::Object(&player)])?;
    post(env, &e)
}

/// `AttackEntityEvent` before the local player attacks entity `id`; true if cancelled.
pub fn attack_entity(env: &mut JNIEnv<'_>, id: i32) -> Result<bool> {
    let Some(player) = the_player(env)? else { return Ok(false) };
    let target = rt::call_static(env, "rustmc/forge/Live", "entity", "(I)Lnet/minecraft/entity/Entity;", &[JValue::Int(id)])?.l()?;
    if target.is_null() { return Ok(false); }
    let e = rt::new_object(env, "net/minecraftforge/event/entity/player/AttackEntityEvent",
        "(Lnet/minecraft/entity/player/EntityPlayer;Lnet/minecraft/entity/Entity;)V", &[JValue::Object(&player), JValue::Object(&target)])?;
    post(env, &e)
}

/// `ClientChatReceivedEvent` for an S02 chat JSON (`kind` 0 chat, 1 system, 2 action bar). `None` if a listener
/// cancelled it, else the message JSON as listeners left it (they may replace `event.message`).
pub fn chat_received(env: &mut JNIEnv<'_>, kind: i8, json: &str) -> Result<Option<String>> {
    const SER: &str = "net/minecraft/util/IChatComponent$Serializer";
    const COMPONENT: &str = "Lnet/minecraft/util/IChatComponent;";
    let text = env.new_string(json)?;
    let component = rt::call_static(env, SER, "func_150699_a", &format!("(Ljava/lang/String;){COMPONENT}"), &[JValue::Object(&text)])?.l()?;
    let e = rt::new_object(env, "net/minecraftforge/client/event/ClientChatReceivedEvent", &format!("(B{COMPONENT})V"), &[JValue::Byte(kind), JValue::Object(&component)])?;
    if post(env, &e)? { return Ok(None); }
    let message = env.get_field(&e, "message", COMPONENT)?.l()?;
    if message.is_null() { return Ok(None); }
    if env.is_same_object(&message, &component)? { return Ok(Some(json.to_string())); }
    let out = rt::call_static(env, SER, "func_150696_a", &format!("({COMPONENT})Ljava/lang/String;"), &[JValue::Object(&message)])?.l()?;
    Ok(Some(env.get_string(&out.into())?.into()))
}

const OVERLAY: &str = "net/minecraftforge/client/event/RenderGameOverlayEvent";

fn overlay_parent<'l>(env: &mut JNIEnv<'l>, partial_ticks: f32) -> Result<JObject<'l>> {
    let mc = minecraft(env)?;
    let sr = rt::new_object(env, "net/minecraft/client/gui/ScaledResolution", &format!("({MC})V"), &[JValue::Object(&mc)])?;
    rt::new_object(env, OVERLAY, "(FLnet/minecraft/client/gui/ScaledResolution;)V", &[JValue::Float(partial_ticks), JValue::Object(&sr)])
}

/// `GuiIngameForge`'s `RenderGameOverlayEvent.Pre` (true = cancelled) or `.Post` for one element (`"ALL"`, `"TEXT"`...).
pub fn overlay(env: &mut JNIEnv<'_>, partial_ticks: f32, pre: bool, element: &str) -> Result<bool> {
    let parent = overlay_parent(env, partial_ticks)?;
    let kind_class = format!("{OVERLAY}$ElementType");
    let kind = env.get_static_field(kind_class.as_str(), element, &format!("L{kind_class};"))?.l()?;
    let class = format!("{OVERLAY}${}", if pre { "Pre" } else { "Post" });
    let e = rt::new_object(env, &class, &format!("(L{OVERLAY};L{kind_class};)V"), &[JValue::Object(&parent), JValue::Object(&kind)])?;
    post(env, &e)
}

/// `RenderGameOverlayEvent.Text`: the HUD's left/right text lines (F3 or empty) after listeners added to them, or
/// `None` if cancelled.
pub fn overlay_text(env: &mut JNIEnv<'_>, partial_ticks: f32, left: &[String], right: &[String]) -> Result<Option<(Vec<String>, Vec<String>)>> {
    fn list<'l>(env: &mut JNIEnv<'l>, lines: &[String]) -> Result<JObject<'l>> {
        let l = rt::new_object(env, "java/util/ArrayList", "()V", &[])?;
        for s in lines { let s = env.new_string(s)?; env.call_method(&l, "add", "(Ljava/lang/Object;)Z", &[JValue::Object(&s)])?; }
        Ok(l)
    }
    let parent = overlay_parent(env, partial_ticks)?;
    let (l, r) = (list(env, left)?, list(env, right)?);
    let e = rt::new_object(env, &format!("{OVERLAY}$Text"), &format!("(L{OVERLAY};Ljava/util/ArrayList;Ljava/util/ArrayList;)V"),
        &[JValue::Object(&parent), JValue::Object(&l), JValue::Object(&r)])?;
    if post(env, &e)? { return Ok(None); }
    let read = |env: &mut JNIEnv<'_>, name: &str| -> Result<Vec<String>> {
        let list = env.get_field(&e, name, "Ljava/util/ArrayList;")?.l()?;
        let n = env.call_method(&list, "size", "()I", &[])?.i()?;
        (0..n).map(|i| {
            let s = env.call_method(&list, "get", "(I)Ljava/lang/Object;", &[JValue::Int(i)])?.l()?;
            Ok(if s.is_null() { String::new() } else { env.get_string(&s.into())?.into() })
        }).collect()
    };
    Ok(Some((read(env, "left")?, read(env, "right")?)))
}

/// `RenderWorldLastEvent` with the camera's matrices loaded (see `impls::begin_world`): what mods draw is projected
/// to GUI units and comes back from `take_draw` as screen-space commands.
pub fn world_last(env: &mut JNIEnv<'_>, partial_ticks: f32, projection: [f64; 16], modelview: [f64; 16], gui: (f64, f64)) -> Result<()> {
    let mc = minecraft(env)?;
    let render_global = env.get_field(&mc, "field_71438_f", "Lnet/minecraft/client/renderer/RenderGlobal;")?.l()?;
    crate::impls::begin_world(projection, modelview, gui);
    let result = rt::new_object(env, "net/minecraftforge/client/event/RenderWorldLastEvent", "(Lnet/minecraft/client/renderer/RenderGlobal;F)V",
        &[JValue::Object(&render_global), JValue::Float(partial_ticks)]).and_then(|e| post(env, &e));
    crate::impls::end_world();
    result.map(drop)
}

/// FML's `InputEvent.KeyInputEvent`, after `Keyboard`'s event state is set (see `impls::set_key`).
pub fn key_input(env: &mut JNIEnv<'_>) -> Result<()> {
    let e = rt::new_object(env, "net/minecraftforge/fml/common/gameevent/InputEvent$KeyInputEvent", "()V", &[])?;
    post(env, &e).map(drop)
}

/// In-game mouse input: Forge's cancellable `MouseEvent` (true = cancelled), then FML's `MouseInputEvent` if not.
/// The event's fields come from `Mouse.getEvent*`, so record the event first.
pub fn mouse_input(env: &mut JNIEnv<'_>) -> Result<bool> {
    let e = rt::new_object(env, "net/minecraftforge/client/event/MouseEvent", "()V", &[])?;
    if post(env, &e)? { return Ok(true); }
    let e = rt::new_object(env, "net/minecraftforge/fml/common/gameevent/InputEvent$MouseInputEvent", "()V", &[])?;
    post(env, &e).map(|_| false)
}

pub fn current_screen<'l>(env: &mut JNIEnv<'l>) -> Result<Option<JObject<'l>>> {
    let mc = minecraft(env)?;
    let s = env.get_field(&mc, "field_71462_r", SCREEN)?.l()?;
    Ok((!s.is_null()).then_some(s))
}

/// `Minecraft.displayGuiScreen`: GuiOpenEvent, then lay out and init the screen (vanilla code posts InitGuiEvent).
pub fn open_screen(env: &mut JNIEnv<'_>, screen: &JObject<'_>) -> Result<()> {
    sync_display(env)?;
    let mc = minecraft(env)?;
    if let Some(old) = current_screen(env)? { env.call_method(&old, "func_146281_b", "()V", &[])?; }
    let mut screen = env.new_local_ref(screen)?;
    if !screen.is_null() {
        let e = rt::new_object(env, "net/minecraftforge/client/event/GuiOpenEvent", &format!("({SCREEN})V"), &[JValue::Object(&screen)])?;
        if post(env, &e)? { return Ok(()); }
        screen = env.get_field(&e, "gui", SCREEN)?.l()?;
    }
    env.set_field(&mc, "field_71462_r", SCREEN, JValue::Object(&screen))?;
    if !screen.is_null() {
        let (w, h) = minecraft_gui_size();
        env.call_method(&screen, "func_146280_a", &format!("({MC}II)V"), &[JValue::Object(&mc), JValue::Int(w), JValue::Int(h)])?;
    }
    Ok(())
}

/// `ForgeHooksClient.drawScreen`: DrawScreenEvent.Pre, the screen, DrawScreenEvent.Post.
pub fn draw_screen(env: &mut JNIEnv<'_>, mouse_x: i32, mouse_y: i32, partial_ticks: f32) -> Result<()> {
    sync_display(env)?;
    let Some(screen) = current_screen(env)? else { return Ok(()) };
    let sig = format!("({SCREEN}IIF)V");
    let args = [JValue::Object(&screen), JValue::Int(mouse_x), JValue::Int(mouse_y), JValue::Float(partial_ticks)];
    let pre = rt::new_object(env, "net/minecraftforge/client/event/GuiScreenEvent$DrawScreenEvent$Pre", &sig, &args)?;
    if post(env, &pre)? { return Ok(()); }
    env.call_method(&screen, "func_73863_a", "(IIF)V", &[JValue::Int(mouse_x), JValue::Int(mouse_y), JValue::Float(partial_ticks)])?;
    let post_event = rt::new_object(env, "net/minecraftforge/client/event/GuiScreenEvent$DrawScreenEvent$Post", &sig, &args)?;
    post(env, &post_event).map(drop)
}

/// A mouse press at GUI coordinates (`button` 0 = left); vanilla `GuiScreen.mouseClicked` fires the ActionPerformed events.
pub fn mouse_click(env: &mut JNIEnv<'_>, x: i32, y: i32, button: i32) -> Result<()> {
    if let Some(screen) = current_screen(env)? {
        env.call_method(&screen, "func_73864_a", "(III)V", &[JValue::Int(x), JValue::Int(y), JValue::Int(button)])?;
    }
    Ok(())
}

/// A typed character with its LWJGL key code (Escape = 1).
pub fn key_typed(env: &mut JNIEnv<'_>, ch: char, key_code: i32) -> Result<()> {
    if let Some(screen) = current_screen(env)? {
        env.call_method(&screen, "func_73869_a", "(CI)V", &[JValue::Char(ch as u16), JValue::Int(key_code)])?;
    }
    Ok(())
}

/// A Java copy of a native client screen, so mods see (and add buttons to) it through `InitGuiEvent`.
static SHADOW: Mutex<Option<GlobalRef>> = Mutex::new(None);

/// What the Java side currently shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus { Nothing, Shadow, Mod }

/// `Minecraft.displayGuiScreen(GuiScreen)` itself, so mods' Mixin handlers around it run before the stub body
/// (`open_screen`) exactly as they would in Java. Calling `open_screen` directly would skip them.
fn display_screen(env: &mut JNIEnv<'_>, screen: &JObject<'_>) -> Result<()> {
    let mc = minecraft(env)?;
    env.call_method(&mc, "func_147108_a", &format!("({SCREEN})V"), &[JValue::Object(screen)]).map(drop)
}

/// Opens the Java twin of a native screen: `"multiplayer"`, `"main_menu"` or `"ingame_menu"`. Mods' GuiOpenEvent and
/// InitGuiEvent handlers run now and can add their buttons to it. Unknown kinds close any shadow.
pub fn open_shadow(env: &mut JNIEnv<'_>, kind: &str) -> Result<()> {
    // A mod opened this vanilla screen itself (`displayGuiScreen(new GuiMultiplayer(this))`) and the client answered
    // with its native one: the mod's object, already shown with its events fired, is the twin.
    if !kind.is_empty() && vanilla_screen(env)? == Some(kind) {
        if let Some(current) = current_screen(env)? { *SHADOW.lock().unwrap() = Some(env.new_global_ref(&current)?); }
        return Ok(());
    }
    let screen = match kind {
        "multiplayer" => rt::new_object(env, "net/minecraft/client/gui/GuiMultiplayer", &format!("({SCREEN})V"), &[JValue::Object(&JObject::null())])?,
        "main_menu" => rt::new_object(env, "net/minecraft/client/gui/GuiMainMenu", "()V", &[])?,
        "ingame_menu" => rt::new_object(env, "net/minecraft/client/gui/GuiIngameMenu", "()V", &[])?,
        _ => return close_shadow(env),
    };
    let before = current_screen(env)?;
    display_screen(env, &screen)?;
    let current = current_screen(env)?;
    let opened = match &current { Some(c) => env.is_same_object(c, &screen)?, None => false };
    *SHADOW.lock().unwrap() = if opened { Some(env.new_global_ref(&screen)?) } else { None };
    if !opened {
        // A Mixin or a GuiOpenEvent handler refused the screen and left the previous one current. The native top
        // screen replaces it, so close the stale Java screen instead of mirroring it back as a mod screen.
        let unchanged = match (&before, &current) {
            (Some(b), Some(c)) => env.is_same_object(b, c)?,
            (None, None) => true,
            _ => false,
        };
        if unchanged { open_screen(env, &JObject::null())?; }
    }
    Ok(())
}

/// Drops the shadow and closes the Java screen through `Minecraft.displayGuiScreen(null)`, so mods' Mixins on it run.
pub fn close_shadow(env: &mut JNIEnv<'_>) -> Result<()> {
    *SHADOW.lock().unwrap() = None;
    display_screen(env, &JObject::null())
}

/// Vanilla screens the client draws natively, by Java class: what a mod's `displayGuiScreen(new GuiOptions(..))`
/// shows. Exact classes only; a mod's subclass is its own screen.
const VANILLA_SCREENS: [(&str, &str); 9] = [
    ("net.minecraft.client.gui.GuiMultiplayer", "multiplayer"),
    ("net.minecraft.client.gui.GuiMainMenu", "main_menu"),
    ("net.minecraft.client.gui.GuiIngameMenu", "ingame_menu"),
    ("net.minecraft.client.gui.GuiOptions", "options"),
    ("net.minecraft.client.gui.GuiVideoSettings", "video"),
    ("net.minecraft.client.gui.GuiControls", "controls"),
    ("net.minecraft.client.gui.GuiScreenResourcePacks", "resource_packs"),
    ("net.minecraft.client.gui.GuiSelectWorld", "select_world"),
    ("net.minecraft.client.gui.GuiLanguage", "language"),
];

/// The native kind of the Java current screen when it is one of [`VANILLA_SCREENS`].
pub fn vanilla_screen(env: &mut JNIEnv<'_>) -> Result<Option<&'static str>> {
    let Some(screen) = current_screen(env)? else { return Ok(None) };
    let class = env.call_method(&screen, "getClass", "()Ljava/lang/Class;", &[])?.l()?;
    let name = env.call_method(&class, "getName", "()Ljava/lang/String;", &[])?.l()?;
    let name: String = env.get_string(&name.into())?.into();
    Ok(VANILLA_SCREENS.iter().find(|(c, _)| *c == name).map(|(_, k)| *k))
}

/// The parent a mod gave the current vanilla screen (`new GuiMultiplayer(this)`): where its Done/Cancel returns.
pub fn vanilla_parent(env: &mut JNIEnv<'_>) -> Result<Option<GlobalRef>> {
    let field = match vanilla_screen(env)? {
        Some("multiplayer") => "field_146798_g",
        Some("options") => "field_146441_g",
        Some("video") => "field_146498_f",
        Some("controls") => "field_146496_h",
        Some("resource_packs") => "field_146965_f",
        Some("select_world") => "field_146632_a",
        Some("language") => "field_146453_a",
        _ => return Ok(None),
    };
    let Some(screen) = current_screen(env)? else { return Ok(None) };
    let parent = env.get_field(&screen, field, SCREEN)?.l()?;
    Ok(if parent.is_null() { None } else { Some(env.new_global_ref(parent)?) })
}

/// `Minecraft.displayGuiScreen(screen)` from the client (a native screen returning to a mod's parent screen).
pub fn show_screen(env: &mut JNIEnv<'_>, screen: &JObject<'_>) -> Result<()> {
    *SHADOW.lock().unwrap() = None;
    display_screen(env, screen)
}

/// What a mod's "Multiplayer" button does: `displayGuiScreen(new GuiMultiplayer(currentScreen))` (capture harness).
pub fn open_multiplayer_from_current(env: &mut JNIEnv<'_>) -> Result<()> {
    let parent = current_screen(env)?.unwrap_or(JObject::null());
    let screen = rt::new_object(env, "net/minecraft/client/gui/GuiMultiplayer", &format!("({SCREEN})V"), &[JValue::Object(&parent)])?;
    display_screen(env, &screen)
}

pub fn focus(env: &mut JNIEnv<'_>) -> Result<Focus> {
    let Some(screen) = current_screen(env)? else { return Ok(Focus::Nothing) };
    let shadow = SHADOW.lock().unwrap().clone();
    Ok(match shadow {
        Some(s) if env.is_same_object(&screen, s.as_obj())? => Focus::Shadow,
        _ => Focus::Mod,
    })
}

/// Window resized: `GuiScreen.onResize`, which re-inits the current screen (and re-fires InitGuiEvent).
pub fn resize(env: &mut JNIEnv<'_>, w: i32, h: i32) -> Result<()> {
    sync_display(env)?;
    if let Some(screen) = current_screen(env)? {
        let mc = minecraft(env)?;
        env.call_method(&screen, "func_175273_b", &format!("({MC}II)V"), &[JValue::Object(&mc), JValue::Int(w), JValue::Int(h)])?;
    }
    Ok(())
}

/// A mouse button released at GUI coordinates.
pub fn mouse_release(env: &mut JNIEnv<'_>, x: i32, y: i32, button: i32) -> Result<()> {
    if let Some(screen) = current_screen(env)? {
        env.call_method(&screen, "func_146286_b", "(III)V", &[JValue::Int(x), JValue::Int(y), JValue::Int(button)])?;
    }
    Ok(())
}

/// A mouse event for the Java screen, as LWJGL would queue it: `button` -1 for a move/wheel, `state` true when pressed,
/// `wheel` in LWJGL notches (120 per click). Positions are GUI coordinates; vanilla `handleMouseInput` (and a mod's override
/// of it, e.g. a GuiSlot's scrolling and dragging) reads them back through `Mouse.getEvent*`.
pub fn mouse_event(env: &mut JNIEnv<'_>, gui_x: i32, gui_y: i32, button: i32, state: bool, wheel: i32) -> Result<()> {
    let Some(screen) = current_screen(env)? else { return Ok(()) };
    sync_display(env)?;
    let d = minecraft_display();
    let (gw, gh) = (d[0].max(1), d[1].max(1));
    // handleMouseInput divides the pixel position by displayWidth/Height again, so pass back framebuffer pixels (y from the bottom).
    set_event(MouseEvent { x: gui_x * d[2] / gw, y: d[3] - 1 - gui_y * d[3] / gh, button, state, wheel });
    env.call_method(&screen, "func_146274_d", "()V", &[])?;
    Ok(())
}

/// `Minecraft.session` as mods currently have it: (username, uuid, access token). Account-switcher mods replace the
/// whole `Session` by reflection, so this is how the client learns who the player is now.
pub fn session(env: &mut JNIEnv<'_>) -> Result<(String, String, String)> {
    let mc = minecraft(env)?;
    let session = env.get_field(&mc, "field_71449_j", "Lnet/minecraft/util/Session;")?.l()?;
    if session.is_null() { return Ok(Default::default()); }
    let text = |env: &mut JNIEnv<'_>, field: &str| -> Result<String> {
        let o = env.get_field(&session, field, "Ljava/lang/String;")?.l()?;
        if o.is_null() { return Ok(String::new()); }
        Ok(env.get_string(&jni::objects::JString::from(o))?.into())
    };
    Ok((text(env, "field_74286_b")?, text(env, "field_148257_b")?, text(env, "field_148258_c")?))
}
