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

/// Opens the Java twin of a native screen: `"multiplayer"`, `"main_menu"` or `"ingame_menu"`. Mods' InitGuiEvent
/// handlers run now and add their buttons to it. Unknown kinds close any shadow.
pub fn open_shadow(env: &mut JNIEnv<'_>, kind: &str) -> Result<()> {
    let screen = match kind {
        "multiplayer" => rt::new_object(env, "net/minecraft/client/gui/GuiMultiplayer", &format!("({SCREEN})V"), &[JValue::Object(&JObject::null())])?,
        "main_menu" => rt::new_object(env, "net/minecraft/client/gui/GuiMainMenu", "()V", &[])?,
        "ingame_menu" => rt::new_object(env, "net/minecraft/client/gui/GuiIngameMenu", "()V", &[])?,
        _ => return close_shadow(env),
    };
    *SHADOW.lock().unwrap() = Some(env.new_global_ref(&screen)?);
    open_screen(env, &screen)
}

/// Drops the shadow (and whatever Java screen is open) without firing open events.
pub fn close_shadow(env: &mut JNIEnv<'_>) -> Result<()> {
    *SHADOW.lock().unwrap() = None;
    open_screen(env, &JObject::null())
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
    let mut text = |env: &mut JNIEnv<'_>, field: &str| -> Result<String> {
        let o = env.get_field(&session, field, "Ljava/lang/String;")?.l()?;
        if o.is_null() { return Ok(String::new()); }
        Ok(env.get_string(&jni::objects::JString::from(o))?.into())
    };
    Ok((text(env, "field_74286_b")?, text(env, "field_148257_b")?, text(env, "field_148258_c")?))
}
