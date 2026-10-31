//! Forge API surface mods reach for LWJGL GL/Display/OpenAL and GlStateManager: methods the corpus scan found unimplemented.
//!
//! `GlStateManager` only caches what GL was told, and gfx's draw sink is the one owner of that cache, so those
//! methods forward to the stubbed `GL11` entry point that changes the sink's state instead of keeping a copy here.
//! What the sink cannot answer (matrices, pixels) is left unread rather than filled with plausible-looking values.
use crate::{runtime as rt, stubs::{register, Ret}};
use jni::objects::JValue;
use std::sync::{atomic::{AtomicI32, Ordering}, Mutex};

const GL11: &str = "org/lwjgl/opengl/GL11";
const GL_FOG: i32 = 0x0b60;
const GL_POLYGON_OFFSET_FILL: i32 = 0x8037;
const ALC_EXTENSIONS: i32 = 0x1006;

pub(crate) fn register_all() {
    gl_state_manager();
    display();
    gl();
    queries();
    openal();
    input();
}

/// `GlStateManager`'s fog, polygon offset, texture-name and display-list caches live in gfx's sink; the `GL11`
/// handler gfx registers is the code that updates them, so these call it (a second dispatch, but one owner).
fn gl_state_manager() {
    const GSM: &str = "net/minecraft/client/renderer/GlStateManager.";
    // disableFog
    register(&format!("{GSM}func_179106_n"), |c| {
        rt::call_static(c.env, GL11, "glDisable", "(I)V", &[JValue::Int(GL_FOG)])?;
        Ok(Ret::Null)
    });
    // disablePolygonOffset
    register(&format!("{GSM}func_179113_r"), |c| {
        rt::call_static(c.env, GL11, "glDisable", "(I)V", &[JValue::Int(GL_POLYGON_OFFSET_FILL)])?;
        Ok(Ret::Null)
    });
    // doPolygonOffset: vanilla skips the GL call when factor and units are unchanged, which only saves a cached write.
    register(&format!("{GSM}func_179136_a"), |c| {
        let (factor, units) = (c.f32(0)?, c.f32(1)?);
        rt::call_static(c.env, GL11, "glPolygonOffset", "(FF)V", &[JValue::Float(factor), JValue::Float(units)])?;
        Ok(Ret::Null)
    });
    // callList
    register(&format!("{GSM}func_179148_o"), |c| {
        let list = c.i32(0)?;
        rt::call_static(c.env, GL11, "glCallList", "(I)V", &[JValue::Int(list)])?;
        Ok(Ret::Null)
    });
    // deleteTexture: the sink also drops the name from the per-unit texture cache, as vanilla does.
    register(&format!("{GSM}func_179150_h"), |c| {
        let texture = c.i32(0)?;
        rt::call_static(c.env, GL11, "glDeleteTextures", "(I)V", &[JValue::Int(texture)])?;
        Ok(Ret::Null)
    });
}

fn display() {
    // The window is the client's, so LWJGL's window hints are ignored (as `setDisplayMode` already is).
    register("org/lwjgl/opengl/Display.setResizable", |_| Ok(Ret::Null));
    register("org/lwjgl/opengl/Display.setLocation", |_| Ok(Ret::Null));
    // The OS window position is not mirrored anywhere in Rust: 0 is what LWJGL's `Display` reports without one.
    register("org/lwjgl/opengl/Display.getX", |_| Ok(0.into()));
    register("org/lwjgl/opengl/Display.getY", |_| Ok(0.into()));
    // The client's one window request is a toggle, and nothing feeds `Minecraft.isFullScreen` when the user
    // toggles fullscreen themselves (`F11`), so toggling on a mod's `true` could leave a fullscreen window
    // windowed. Ignored until the client can be asked for an absolute state.
    register("org/lwjgl/opengl/Display.setFullscreen(Z)V", |_| Ok(Ret::Null));
}

fn gl() {
    // The sink culls nothing, so a winding has nothing to orient; the cached state had no other reader in vanilla.
    register(&format!("{GL11}.glFrontFace"), |_| Ok(Ret::Null));
    register(&format!("{GL11}.glGetTexImage(IIIILjava/nio/IntBuffer;)V"), |_| {
        note("GL11.glGetTexImage", "textures are the client's GPU uploads; no pixel copy is kept to return");
        Ok(Ret::Null)
    });
    register(&format!("{GL11}.glReadPixels(IIIIIILjava/nio/IntBuffer;)V"), |_| {
        note("GL11.glReadPixels", "the frame is the client's renderer's; there are no pixels to read back");
        Ok(Ret::Null)
    });
}

/// Names handed out by `glGenQueries`, from 1 (0 is LWJGL's "allocation failed").
static QUERIES: AtomicI32 = AtomicI32::new(1);

fn queries() {
    const GL15: &str = "org/lwjgl/opengl/GL15";
    // Unique names like a driver's; nothing else is remembered, because the sink rasterises nothing for a query
    // to count (see `glGetQueryObjecti`).
    register(&format!("{GL15}.glGenQueries()I"), |_| Ok(QUERIES.fetch_add(1, Ordering::Relaxed).into()));
    register(&format!("{GL15}.glBeginQuery"), |_| Ok(Ret::Null));
    register(&format!("{GL15}.glEndQuery"), |_| Ok(Ret::Null));
    register(&format!("{GL15}.glDeleteQueries"), |_| Ok(Ret::Null));
    // GL_QUERY_RESULT, GL_QUERY_RESULT_AVAILABLE and GL_SAMPLES_PASSED all answer 0: the sink rasterises nothing,
    // so no query ever completes, and 0 for `..._AVAILABLE` is exactly "no result yet" (LWJGL's own default). A
    // culling mod then keeps drawing what it cannot verify instead of hiding all of it.
    register(&format!("{GL15}.glGetQueryObjecti"), |_| Ok(0.into()));
}

fn openal() {
    // The mixer is the client's and always up, like `Display.isCreated`: there is no context for a mod to open or
    // tear down, and the client's sound is not a mod's to destroy.
    register("org/lwjgl/openal/AL.isCreated", |_| Ok(true.into()));
    register("org/lwjgl/openal/AL.create", |_| Ok(Ret::Null));
    register("org/lwjgl/openal/AL.destroy", |_| Ok(Ret::Null));
    // No ALC device is exposed: a query for supported extensions has an answer (none), a device or default-device
    // name has none, which LWJGL hands back as null.
    register("org/lwjgl/openal/ALC10.alcGetString", |c| {
        Ok(match c.i32(1)? {
            ALC_EXTENSIONS => String::new().into(),
            _ => Ret::Null,
        })
    });
}

fn input() {
    // Input reaches the client, so the keyboard exists (`Display.isCreated` is true for the same reason).
    register("org/lwjgl/input/Keyboard.isCreated", |_| Ok(true.into()));
    // The cursor is the client's: `Mouse.getX/getY` report where its events put the pointer, and nothing here can
    // warp the OS cursor into the window.
    register("org/lwjgl/input/Mouse.setCursorPosition", |_| Ok(Ret::Null));
}

/// Says why a call has no data to hand back, once per name: the generic stub log would say "unimplemented", which
/// these are not, and a mod asking every frame should not flood the log.
fn note(name: &'static str, why: &'static str) {
    static SAID: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let mut said = SAID.lock().unwrap();
    if !said.contains(&name) {
        said.push(name);
        eprintln!("[forge] {name}: {why}");
    }
}
