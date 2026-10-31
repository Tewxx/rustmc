//! `Minecraft`: the singleton, its screen and the services real GUI code reaches through it.
use crate::{game, stubs::{register, Ret}};
use crate::runtime as rt;
use jni::{errors::Result, objects::{GlobalRef, JObject, JValue}, JNIEnv};
use std::sync::{Mutex, OnceLock};

const MC: &str = "net/minecraft/client/Minecraft.";
const SCREEN: &str = "Lnet/minecraft/client/gui/GuiScreen;";

static INSTANCE: OnceLock<GlobalRef> = OnceLock::new();
/// (GUI-scaled width, height, framebuffer width, height)
static DISPLAY: Mutex<[i32; 4]> = Mutex::new([427, 240, 854, 480]);
static SESSION: Mutex<(String, String)> = Mutex::new((String::new(), String::new()));
static FPS: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
static FULLSCREEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The client's frame rate, for `Minecraft.getDebugFPS` (`F3` overlays and mods read it).
pub fn set_fps(fps: i32) { FPS.store(fps, std::sync::atomic::Ordering::Relaxed); }
/// The client's window state, for `Minecraft.isFullScreen`.
pub fn set_fullscreen(fullscreen: bool) { FULLSCREEN.store(fullscreen, std::sync::atomic::Ordering::Relaxed); }

/// Sets the sizes mods see: the GUI-scaled size screens lay out in, and the framebuffer size.
pub fn set_display(gui: (i32, i32), framebuffer: (i32, i32)) {
    let old = std::mem::replace(&mut *DISPLAY.lock().unwrap(), [gui.0, gui.1, framebuffer.0, framebuffer.1]);
    // Minecraft.resize/updateDisplay: the viewport follows the window.
    if (old[2], old[3]) != framebuffer { super::gfx::reset_viewport(framebuffer); }
}

/// The player name and UUID mods see. The access token is never handed out (it stays "0").
pub fn set_session(name: &str, uuid: &str) { *SESSION.lock().unwrap() = (name.into(), uuid.into()); }

pub(crate) fn framebuffer_size() -> (i32, i32) { let d = DISPLAY.lock().unwrap(); (d[2], d[3]) }

pub(crate) fn display() -> [i32; 4] { *DISPLAY.lock().unwrap() }

pub(crate) fn gui_size() -> (i32, i32) { let d = DISPLAY.lock().unwrap(); (d[0], d[1]) }

/// The shared `Minecraft`, built on first use: font renderer, data dir and a token-less session.
pub fn minecraft<'l>(env: &mut JNIEnv<'l>) -> Result<JObject<'l>> {
    if INSTANCE.get().is_none() {
        let mc = rt::alloc(env, "net/minecraft/client/Minecraft")?;
        let font = rt::alloc(env, "net/minecraft/client/gui/FontRenderer")?;
        env.set_field(&font, "field_78288_b", "I", JValue::Int(9))?;
        env.set_field(&mc, "field_71466_p", "Lnet/minecraft/client/gui/FontRenderer;", JValue::Object(&font))?;
        let d = *DISPLAY.lock().unwrap();
        env.set_field(&mc, "field_71443_c", "I", JValue::Int(d[2]))?;
        env.set_field(&mc, "field_71440_d", "I", JValue::Int(d[3]))?;
        // Real GUI code reads gameSettings (touchscreen, guiScale...); an all-zero object means vanilla defaults off.
        let settings = rt::alloc(env, "net/minecraft/client/settings/GameSettings")?;
        env.set_field(&mc, "field_71474_y", "Lnet/minecraft/client/settings/GameSettings;", JValue::Object(&settings))?;
        let dir = env.new_string(".")?;
        let dir = rt::new_object(env, "java/io/File", "(Ljava/lang/String;)V", &[JValue::Object(&dir)])?;
        env.set_field(&mc, "field_71412_D", "Ljava/io/File;", JValue::Object(&dir))?;
        let (name, uuid) = SESSION.lock().unwrap().clone();
        let name = env.new_string(if name.is_empty() { "Player" } else { &name })?;
        let uuid = env.new_string(uuid)?;
        let token = env.new_string("0")?;
        let kind = env.new_string("mojang")?;
        let session = rt::new_object(env, "net/minecraft/util/Session", "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
            &[JValue::Object(&name), JValue::Object(&uuid), JValue::Object(&token), JValue::Object(&kind)])?;
        env.set_field(&mc, "field_71449_j", "Lnet/minecraft/util/Session;", JValue::Object(&session))?;
        let _ = INSTANCE.set(env.new_global_ref(mc)?);
    }
    env.new_local_ref(INSTANCE.get().unwrap().as_obj())
}

pub fn register_all() {
    let k = |name: &str| format!("{MC}{name}");
    register(&k("func_71410_x"), |c| { let mc = minecraft(c.env)?; Ok(Ret::Obj(c.env.new_global_ref(mc)?)) });
    register(&k("func_110432_I"), |c| {
        let mc = minecraft(c.env)?;
        let session = c.env.get_field(&mc, "field_71449_j", "Lnet/minecraft/util/Session;")?.l()?;
        Ok(Ret::Obj(c.env.new_global_ref(session)?))
    });
    register(&k("func_147108_a"), |c| { let screen = c.obj(0)?; game::open_screen(c.env, &screen)?; Ok(Ret::Null) });
    // Work queued from other threads runs inline: there is no separate game thread yet.
    register(&k("func_152344_a"), |c| {
        let task = c.obj(0)?;
        c.env.call_method(&task, "run", "()V", &[])?;
        immediate(c.env, JObject::null())
    });
    register(&k("func_152343_a"), |c| {
        let task = c.obj(0)?;
        let value = c.env.call_method(&task, "call", "()Ljava/lang/Object;", &[])?.l()?;
        immediate(c.env, value)
    });
    // Services real GUI code reaches through the singleton; their methods are stubs (no sound/texture backend yet).
    for (name, class) in [("func_147118_V", "net/minecraft/client/audio/SoundHandler"), ("func_110434_K", "net/minecraft/client/renderer/texture/TextureManager"),
                          ("func_175599_af", "net/minecraft/client/renderer/entity/RenderItem"),
                          ("func_152342_ad", "net/minecraft/client/resources/SkinManager")] {
        register(&k(name), move |c| {
            static CELLS: Mutex<Vec<(&str, GlobalRef)>> = Mutex::new(Vec::new());
            let found = CELLS.lock().unwrap().iter().find(|(n, _)| *n == class).map(|(_, g)| g.clone());
            let obj = match found {
                Some(g) => g,
                None => { let o = rt::alloc(c.env, class)?; let g = c.env.new_global_ref(o)?; CELLS.lock().unwrap().push((class, g.clone())); g }
            };
            Ok(Ret::Obj(obj))
        });
    }
    register(&k("func_110442_L"), |c| {
        let manager = rt::static_field(c.env, "rustmc/forge/Resources", "INSTANCE", "Lrustmc/forge/Resources;")?.l()?;
        Ok(Ret::Obj(c.env.new_global_ref(manager)?))
    });
    register("net/minecraft/util/Util.func_110647_a", |c| {
        let os = if cfg!(target_os = "macos") { "OSX" } else if cfg!(target_os = "windows") { "WINDOWS" } else if cfg!(target_os = "linux") { "LINUX" } else { "UNKNOWN" };
        let v = rt::static_field(c.env, "net/minecraft/util/Util$EnumOS", os, "Lnet/minecraft/util/Util$EnumOS;")?.l()?;
        Ok(Ret::Obj(c.env.new_global_ref(v)?))
    });
    // Multiplayer only: no integrated server; the current server is unknown until the net layer reports it.
    register(&k("func_147104_D"), |_| Ok(Ret::Null));
    register(&k("func_71387_A"), |_| Ok(false.into()));
    register(&k("func_71356_B"), |_| Ok(false.into()));
    register(&k("func_175600_c"), |_| Ok("1.8.9".into()));
    register(&k("func_71386_F"), |_| {
        Ok((std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)).into())
    });
    minecraft_window();
    let _ = SCREEN;
}

/// The singleton's own state and window: the Rust client owns the window, so changes are queued for it.
fn minecraft_window() {
    fn k(name: &str) -> String { format!("net/minecraft/client/Minecraft.{name}") }
    // getFramebuffer: one Framebuffer, kept in framebufferMc the way vanilla keeps it there.
    register(&k("func_147110_a"), |c| {
        let existing = c.env.get_field(&c.this, "field_147124_at", "Lnet/minecraft/client/shader/Framebuffer;")?.l()?;
        let buffer = if existing.is_null() { rt::alloc(c.env, "net/minecraft/client/shader/Framebuffer")? } else { existing };
        c.env.set_field(&c.this, "field_147124_at", "Lnet/minecraft/client/shader/Framebuffer;", JValue::Object(&buffer))?;
        Ok(Ret::Obj(c.env.new_global_ref(buffer)?))
    });
    // resize: the display size is the client's; keep the mirrored fields in step and let it resize the window.
    register(&k("func_71370_a"), |c| {
        let (width, height) = (c.i32(0)?, c.i32(1)?);
        c.env.set_field(&c.this, "field_71443_c", "I", JValue::Int(width))?;
        c.env.set_field(&c.this, "field_71440_d", "I", JValue::Int(height))?;
        crate::impls::client::push_resize(width, height);
        Ok(Ret::Null)
    });
    register(&k("func_71400_g"), |_| { crate::impls::client::push_shutdown(); Ok(Ret::Null) });
    // Multiplayer never pauses: vanilla only sets isGamePaused for an integrated server's pause menu.
    register(&k("func_147113_T"), |_| Ok(false.into()));
    register(&k("func_175610_ah"), |_| Ok(FPS.load(std::sync::atomic::Ordering::Relaxed).into()));
    register(&k("func_71372_G"), |_| Ok(FULLSCREEN.load(std::sync::atomic::Ordering::Relaxed).into()));
    register(&k("func_71352_k"), |_| {
        let full = !FULLSCREEN.load(std::sync::atomic::Ordering::Relaxed);
        FULLSCREEN.store(full, std::sync::atomic::Ordering::Relaxed);
        crate::impls::client::push_toggle_fullscreen();
        Ok(Ret::Null)
    });
    // isGuiEnabled: static in vanilla, so read gameSettings off the singleton rather than `this`.
    register(&k("func_71382_s"), |c| {
        let mc = minecraft(c.env)?;
        let settings = c.env.get_field(&mc, "field_71474_y", "Lnet/minecraft/client/settings/GameSettings;")?.l()?;
        if settings.is_null() { return Ok(true.into()); }
        Ok((!c.env.get_field(&settings, "field_74319_N", "Z")?.z()?).into())
    });
}

fn immediate(env: &mut JNIEnv<'_>, value: JObject<'_>) -> Result<Ret> {
    let f = rt::call_static(env, "com/google/common/util/concurrent/Futures", "immediateFuture",
        "(Ljava/lang/Object;)Lcom/google/common/util/concurrent/ListenableFuture;", &[JValue::Object(&value)])?.l()?;
    Ok(Ret::Obj(env.new_global_ref(f)?))
}
