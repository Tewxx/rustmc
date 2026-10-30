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

/// Sets the sizes mods see: the GUI-scaled size screens lay out in, and the framebuffer size.
pub fn set_display(gui: (i32, i32), framebuffer: (i32, i32)) { *DISPLAY.lock().unwrap() = [gui.0, gui.1, framebuffer.0, framebuffer.1]; }

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
                          ("func_175599_af", "net/minecraft/client/renderer/entity/RenderItem")] {
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
    let _ = SCREEN;
}

fn immediate(env: &mut JNIEnv<'_>, value: JObject<'_>) -> Result<Ret> {
    let f = rt::call_static(env, "com/google/common/util/concurrent/Futures", "immediateFuture",
        "(Ljava/lang/Object;)Lcom/google/common/util/concurrent/ListenableFuture;", &[JValue::Object(&value)])?.l()?;
    Ok(Ret::Obj(env.new_global_ref(f)?))
}
