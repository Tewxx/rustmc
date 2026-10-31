//! LWJGL input state and `ScaledResolution`, fed by the client (`set_mouse`) and the display size.
use super::minecraft::{framebuffer_size, gui_size};
use crate::stubs::{register, Call, Ret};
use jni::{errors::Result, objects::{JObject, JValue}, JNIEnv};
use std::sync::Mutex;

type ClipGet = Box<dyn Fn() -> String + Send>;
type ClipSet = Box<dyn Fn(&str) + Send>;
static CLIPBOARD: Mutex<Option<(ClipGet, ClipSet)>> = Mutex::new(None);

/// The system clipboard for `GuiScreen.get/setClipboardString` (vanilla uses AWT, which is off: see `runtime`).
pub fn set_clipboard(get: impl Fn() -> String + Send + 'static, set: impl Fn(&str) + Send + 'static) {
    *CLIPBOARD.lock().unwrap() = Some((Box::new(get), Box::new(set)));
}

/// The pointer as LWJGL sees it: position in framebuffer pixels from the top, motion and wheel accumulated since
/// `getDX`/`getDY`/`getDWheel` last read them, and the grab flag.
#[derive(Clone, Copy)]
struct Mouse { x: i32, y: i32, dx: i32, dy: i32, wheel: i32, grabbed: bool }
static MOUSE: Mutex<Mouse> = Mutex::new(Mouse { x: 0, y: 0, dx: 0, dy: 0, wheel: 0, grabbed: true });
static MODS: Mutex<[bool; 3]> = Mutex::new([false; 3]);
/// `Keyboard.enableRepeatEvents`, and the title mods set on the client's window.
static REPEAT: Mutex<bool> = Mutex::new(false);
static TITLE: Mutex<Option<String>> = Mutex::new(None);
const DEFAULT_TITLE: &str = "Minecraft 1.8.9";

/// The current `Mouse.getEvent*` event plus held buttons, set by `game::mouse_event`.
#[derive(Clone, Copy, Default)]
pub(crate) struct MouseEvent { pub x: i32, pub y: i32, pub button: i32, pub state: bool, pub wheel: i32 }
static EVENT: Mutex<MouseEvent> = Mutex::new(MouseEvent { x: 0, y: 0, button: -1, state: false, wheel: 0 });
static HELD: Mutex<[bool; 3]> = Mutex::new([false; 3]);

/// Record one mouse event (framebuffer pixels, y from the bottom) before calling `handleMouseInput`.
pub(crate) fn set_event(e: MouseEvent) {
    if (0..3).contains(&e.button) { HELD.lock().unwrap()[e.button as usize] = e.state; }
    if e.wheel != 0 { let mut m = MOUSE.lock().unwrap(); m.wheel = m.wheel.saturating_add(e.wheel); }
    *EVENT.lock().unwrap() = e;
}

/// Ctrl, Shift and Alt as `Keyboard.isKeyDown` reports them (GuiScreen's ctrl+C/V/A checks read these).
pub fn set_modifiers(ctrl: bool, shift: bool, alt: bool) { *MODS.lock().unwrap() = [ctrl, shift, alt]; }

/// Pointer position in framebuffer pixels, y measured from the top (the client's window coordinates).
pub fn set_mouse(x: i32, y: i32) {
    let mut m = MOUSE.lock().unwrap();
    m.dx = m.dx.saturating_add(x - m.x);
    m.dy = m.dy.saturating_add(m.y - y); // LWJGL's y axis points up
    (m.x, m.y) = (x, y);
}

/// Held LWJGL key codes, and the current `Keyboard.getEvent*` event: (key, pressed, character, repeat).
static KEYS: Mutex<[bool; 256]> = Mutex::new([false; 256]);
static KEY_EVENT: Mutex<(i32, bool, u16, bool)> = Mutex::new((0, false, 0, false));

/// Record one keyboard event (LWJGL code, `char` 0 when none) before posting `KeyInputEvent` or calling a screen.
pub fn set_key(code: i32, down: bool, ch: Option<char>, repeat: bool) {
    if (0..256).contains(&code) { KEYS.lock().unwrap()[code as usize] = down; }
    *KEY_EVENT.lock().unwrap() = (code, down, ch.map_or(0, |c| c as u32 as u16), repeat);
}

/// Releases every held key, as LWJGL reports when the window loses focus.
pub fn release_keys() { *KEYS.lock().unwrap() = [false; 256]; }

/// Record an in-game mouse event at the current pointer (framebuffer pixels, y from the bottom).
pub fn set_mouse_button(button: i32, state: bool, wheel: i32) {
    let (x, y) = { let m = *MOUSE.lock().unwrap(); (m.x, m.y) };
    set_event(MouseEvent { x, y: framebuffer_size().1 - 1 - y, button, state, wheel });
}

fn nanos() -> i64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_nanos() as i64
}

/// Every `KeyBinding` mods constructed; the generated stub class never runs vanilla's static registry.
static BINDINGS: Mutex<Vec<jni::objects::GlobalRef>> = Mutex::new(Vec::new());
const KB: &str = "net/minecraft/client/settings/KeyBinding";

/// `KeyBinding.setKeyBindState` and, on a press, `KeyBinding.onTick` for every binding on `code` (mouse buttons are
/// `button - 100`), as `Minecraft.runTick` does while no screen is open.
pub fn key_bindings(env: &mut jni::JNIEnv<'_>, code: i32, down: bool) -> jni::errors::Result<()> {
    if code == 0 { return Ok(()); }
    let bindings: Vec<_> = BINDINGS.lock().unwrap().clone();
    for b in bindings {
        if env.get_field(&b, "field_74512_d", "I")?.i()? != code { continue; }
        env.set_field(&b, "field_74513_e", "Z", jni::objects::JValue::Bool(down.into()))?;
        if down {
            let n = env.get_field(&b, "field_151474_i", "I")?.i()?;
            env.set_field(&b, "field_151474_i", "I", jni::objects::JValue::Int(n + 1))?;
        }
    }
    Ok(())
}

/// `KeyBinding.unPressAllKeys`, when a screen opens.
pub fn unpress_key_bindings(env: &mut jni::JNIEnv<'_>) -> jni::errors::Result<()> {
    let bindings: Vec<_> = BINDINGS.lock().unwrap().clone();
    for b in bindings {
        env.set_field(&b, "field_74513_e", "Z", jni::objects::JValue::Bool(0))?;
        env.set_field(&b, "field_151474_i", "I", jni::objects::JValue::Int(0))?;
    }
    Ok(())
}

fn register_key_bindings() {
    let k = |m: &str| format!("{KB}.{m}");
    register(&k("<init>"), |c| {
        let (desc, code, cat) = (c.obj(0)?, c.i32(1)?, c.obj(2)?);
        let this = c.env.new_local_ref(&c.this)?;
        c.env.set_field(&this, "field_74515_c", "Ljava/lang/String;", JValue::Object(&desc))?;
        c.env.set_field(&this, "field_151471_f", "Ljava/lang/String;", JValue::Object(&cat))?;
        c.env.set_field(&this, "field_74512_d", "I", JValue::Int(code))?;
        c.env.set_field(&this, "field_151472_e", "I", JValue::Int(code))?;
        BINDINGS.lock().unwrap().push(c.env.new_global_ref(&this)?);
        Ok(Ret::Null)
    });
    register(&k("func_151470_d"), |c| Ok(c.env.get_field(&c.this, "field_74513_e", "Z")?.z()?.into()));
    // isPressed consumes one queued press.
    register(&k("func_151468_f"), |c| {
        let n = c.env.get_field(&c.this, "field_151474_i", "I")?.i()?;
        if n == 0 { return Ok(false.into()); }
        c.env.set_field(&c.this, "field_151474_i", "I", JValue::Int(n - 1))?;
        Ok(true.into())
    });
    register(&k("func_151463_i"), |c| Ok(c.env.get_field(&c.this, "field_74512_d", "I")?.i()?.into()));
    register(&k("func_151469_h"), |c| Ok(c.env.get_field(&c.this, "field_151472_e", "I")?.i()?.into()));
    register(&k("func_151462_b"), |c| { let v = c.i32(0)?; c.env.set_field(&c.this, "field_74512_d", "I", JValue::Int(v))?; Ok(Ret::Null) });
    for (method, field) in [("func_151464_g", "field_74515_c"), ("func_151466_e", "field_151471_f")] {
        register(&k(method), move |c| { let s = c.env.get_field(&c.this, field, "Ljava/lang/String;")?.l()?; Ok(Ret::Obj(c.env.new_global_ref(s)?)) });
    }
    register(&k("func_74507_a"), |c| { let code = c.i32(0)?; key_bindings(c.env, code, true).map(|_| Ret::Null) });
    register(&k("func_74510_a"), |c| {
        let (code, down) = (c.i32(0)?, c.bool(1)?);
        let bindings: Vec<_> = BINDINGS.lock().unwrap().clone();
        for b in bindings {
            if c.env.get_field(&b, "field_74512_d", "I")?.i()? == code { c.env.set_field(&b, "field_74513_e", "Z", JValue::Bool(down.into()))?; }
        }
        Ok(Ret::Null)
    });
    register(&k("func_74506_a"), |c| unpress_key_bindings(c.env).map(|_| Ret::Null));
    register(&k("func_74508_b"), |_| Ok(Ret::Null));
    // The constructor already records the binding; the controls screen list is the client's own.
    register("net/minecraftforge/fml/client/registry/ClientRegistry.registerKeyBinding", |_| Ok(Ret::Null));
}

fn open_url(url: &str) -> std::io::Result<std::process::Child> {
    use std::process::Command;
    if cfg!(target_os = "macos") { Command::new("open").arg(url).spawn() }
    else if cfg!(target_os = "windows") { Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn() }
    else { Command::new("xdg-open").arg(url).spawn() }
}

pub fn register_all() {
    register("org/lwjgl/input/Mouse.getX", |_| Ok(MOUSE.lock().unwrap().x.into()));
    // LWJGL's origin is the bottom-left corner.
    register("org/lwjgl/input/Mouse.getY", |_| Ok((framebuffer_size().1 - 1 - MOUSE.lock().unwrap().y).into()));
    // Deltas and wheel are consumed by the read, as LWJGL's cached values are.
    register("org/lwjgl/input/Mouse.getDX", |_| { let mut m = MOUSE.lock().unwrap(); Ok(std::mem::take(&mut m.dx).into()) });
    register("org/lwjgl/input/Mouse.getDY", |_| { let mut m = MOUSE.lock().unwrap(); Ok(std::mem::take(&mut m.dy).into()) });
    register("org/lwjgl/input/Mouse.getDWheel", |_| { let mut m = MOUSE.lock().unwrap(); Ok(std::mem::take(&mut m.wheel).into()) });
    register("org/lwjgl/input/Mouse.isGrabbed", |_| Ok(MOUSE.lock().unwrap().grabbed.into()));
    register("org/lwjgl/input/Mouse.setGrabbed", |c| {
        MOUSE.lock().unwrap().grabbed = c.bool(0)?;
        Ok(Ret::Null)
    });
    register("org/lwjgl/input/Keyboard.isKeyDown", |c| {
        let m = *MODS.lock().unwrap();
        let code = c.i32(0)?;
        let held = (0..256).contains(&code) && KEYS.lock().unwrap()[code as usize];
        Ok((held || match code { 29 | 157 => m[0], 42 | 54 => m[1], 56 | 184 => m[2], _ => false }).into())
    });
    register("org/lwjgl/input/Keyboard.getEventKey", |_| Ok(KEY_EVENT.lock().unwrap().0.into()));
    register("org/lwjgl/input/Keyboard.getEventKeyState", |_| Ok(KEY_EVENT.lock().unwrap().1.into()));
    register("org/lwjgl/input/Keyboard.getEventCharacter", |_| Ok((KEY_EVENT.lock().unwrap().2 as i32).into()));
    register("org/lwjgl/input/Keyboard.isRepeatEvent", |_| Ok(KEY_EVENT.lock().unwrap().3.into()));
    register("org/lwjgl/input/Mouse.getEventDX", |_| Ok(0.into()));
    register("org/lwjgl/input/Mouse.getEventDY", |_| Ok(0.into()));
    register("org/lwjgl/input/Mouse.getEventNanoseconds", |_| Ok(Ret::Long(nanos())));
    register_key_bindings();
    register("org/lwjgl/input/Mouse.getEventX", |_| Ok(EVENT.lock().unwrap().x.into()));
    register("org/lwjgl/input/Mouse.getEventY", |_| Ok(EVENT.lock().unwrap().y.into()));
    register("org/lwjgl/input/Mouse.getEventButton", |_| Ok(EVENT.lock().unwrap().button.into()));
    register("org/lwjgl/input/Mouse.getEventButtonState", |_| Ok(EVENT.lock().unwrap().state.into()));
    register("org/lwjgl/input/Mouse.getEventDWheel", |_| Ok(EVENT.lock().unwrap().wheel.into()));
    register("org/lwjgl/input/Mouse.isButtonDown", |c| { let b = c.i32(0)?; Ok((0..3).contains(&b).then(|| HELD.lock().unwrap()[b as usize]).unwrap_or(false).into()) });
    register("org/lwjgl/input/Keyboard.getKeyName", |c| {
        let name = key_name(c.i32(0)?);
        Ok(name.map_or(Ret::Null, |n| Ret::Str(n.to_string())))
    });
    register("org/lwjgl/input/Keyboard.getKeyIndex", |c| Ok(key_index(&c.string(0)?.unwrap_or_default()).into()));
    register("org/lwjgl/input/Keyboard.getKeyCount", |_| Ok(256.into()));
    register("org/lwjgl/input/Keyboard.enableRepeatEvents", |c| {
        *REPEAT.lock().unwrap() = c.bool(0)?;
        Ok(Ret::Null)
    });
    register("org/lwjgl/input/Keyboard.areRepeatEventsEnabled", |_| Ok(REPEAT.lock().unwrap().to_owned().into()));
    // The client owns the window, so `Display` reports its framebuffer and ignores what it cannot change.
    register("org/lwjgl/opengl/Display.getWidth", |_| Ok(framebuffer_size().0.into()));
    register("org/lwjgl/opengl/Display.getHeight", |_| Ok(framebuffer_size().1.into()));
    register("org/lwjgl/opengl/Display.isCreated", |_| Ok(true.into()));
    register("org/lwjgl/opengl/Display.isActive", |_| Ok(true.into()));
    register("org/lwjgl/opengl/Display.isVisible", |_| Ok(true.into()));
    register("org/lwjgl/opengl/Display.update", |_| Ok(Ret::Null));
    register("org/lwjgl/opengl/Display.setDisplayMode", |_| Ok(Ret::Null));
    register("org/lwjgl/opengl/Display.getTitle", |_| Ok(Ret::Str(TITLE.lock().unwrap().clone().unwrap_or_else(|| DEFAULT_TITLE.to_string()))));
    register("org/lwjgl/opengl/Display.setTitle", |c| {
        *TITLE.lock().unwrap() = c.string(0)?;
        Ok(Ret::Null)
    });
    register("org/lwjgl/opengl/Display.getDisplayMode", |c| mode(c.env, framebuffer_size()));
    register("org/lwjgl/opengl/Display.getDesktopDisplayMode", |c| mode(c.env, framebuffer_size()));
    register("org/lwjgl/opengl/Display.getAvailableDisplayModes", |c| {
        let arr: JObject = c.env.new_object_array(0, MODE, JObject::null())?.into();
        Ok(Ret::Obj(c.env.new_global_ref(&arr)?))
    });
    // `DisplayMode` keeps its own fields, so mods that build one read back what they passed.
    register("org/lwjgl/opengl/DisplayMode.<init>", |c| {
        let (w, h) = (c.i32(0)?, c.i32(1)?);
        c.env.set_field(&c.this, "width", "I", JValue::Int(w))?;
        c.env.set_field(&c.this, "height", "I", JValue::Int(h))?;
        Ok(Ret::Null)
    });
    register(&format!("{MODE}.getWidth"), |c| Ok(c.env.get_field(&c.this, "width", "I")?.i()?.into()));
    register(&format!("{MODE}.getHeight"), |c| Ok(c.env.get_field(&c.this, "height", "I")?.i()?.into()));
    register("org/lwjgl/BufferUtils.createByteBuffer", |c| {
        let cap = c.i32(0)?;
        let buf = direct(c.env, cap)?;
        Ok(Ret::Obj(c.env.new_global_ref(&buf)?))
    });
    register("org/lwjgl/BufferUtils.createIntBuffer", |c| view(c, "asIntBuffer", "()Ljava/nio/IntBuffer;"));
    register("org/lwjgl/BufferUtils.createFloatBuffer", |c| view(c, "asFloatBuffer", "()Ljava/nio/FloatBuffer;"));
    register("org/lwjgl/BufferUtils.createShortBuffer", |c| view(c, "asShortBuffer", "()Ljava/nio/ShortBuffer;"));
    register("org/lwjgl/BufferUtils.createDoubleBuffer", |c| view(c, "asDoubleBuffer", "()Ljava/nio/DoubleBuffer;"));
    register("net/minecraft/client/gui/GuiScreen.func_146277_j", |_| Ok(CLIPBOARD.lock().unwrap().as_ref().map_or(String::new(), |(get, _)| get()).into()));
    register("net/minecraft/client/gui/GuiScreen.func_146275_d", |c| {
        let text = c.string(0)?.unwrap_or_default();
        if let Some((_, set)) = CLIPBOARD.lock().unwrap().as_ref() { set(&text); }
        Ok(().into())
    });
    // AwtShim (see rustmc.boot.AwtTransformer): mods' AWT clipboard calls land here.
    register("rustmc/forge/AwtShim.setClipboard", |c| {
        let text = c.string(0)?.unwrap_or_default();
        if let Some((_, set)) = CLIPBOARD.lock().unwrap().as_ref() { set(&text); }
        Ok(().into())
    });
    register("rustmc/forge/AwtShim.getClipboard", |_| Ok(CLIPBOARD.lock().unwrap().as_ref().map_or(Ret::Null, |(get, _)| get().into())));
    // Only web links, and only through the OS opener: mods never get to start arbitrary processes.
    register("rustmc/forge/AwtShim.browse", |c| {
        let url = c.string(0)?.unwrap_or_default();
        if url.starts_with("https://") || url.starts_with("http://") {
            eprintln!("[forge] opening {url}");
            let _ = open_url(&url);
        } else {
            eprintln!("[forge] refused to open non-web link {url}");
        }
        Ok(().into())
    });
    const SR: &str = "net/minecraft/client/gui/ScaledResolution.";
    register(&format!("{SR}<init>"), |_| Ok(().into()));
    register(&format!("{SR}func_78326_a"), |_| Ok(gui_size().0.into()));
    register(&format!("{SR}func_78328_b"), |_| Ok(gui_size().1.into()));
    register(&format!("{SR}func_78327_c"), |_| Ok((gui_size().0 as f64).into()));
    register(&format!("{SR}func_78324_d"), |_| Ok((gui_size().1 as f64).into()));
    register(&format!("{SR}func_78325_e"), |_| Ok((framebuffer_size().0 / gui_size().0.max(1)).max(1).into()));
}

const MODE: &str = "org/lwjgl/opengl/DisplayMode";

/// A `DisplayMode` of the given size; the stub constructor is what fills in its fields.
fn mode(env: &mut JNIEnv<'_>, (w, h): (i32, i32)) -> Result<Ret> {
    let m = env.new_object(MODE, "(II)V", &[JValue::Int(w), JValue::Int(h)])?;
    Ok(Ret::Obj(env.new_global_ref(&m)?))
}

/// A direct `ByteBuffer` in native byte order, as `BufferUtils` allocates them.
fn direct<'l>(env: &mut JNIEnv<'l>, cap: i32) -> Result<JObject<'l>> {
    let bb = env.call_static_method("java/nio/ByteBuffer", "allocateDirect", "(I)Ljava/nio/ByteBuffer;", &[JValue::Int(cap)])?.l()?;
    let order = env.call_static_method("java/nio/ByteOrder", "nativeOrder", "()Ljava/nio/ByteOrder;", &[])?.l()?;
    Ok(env.call_method(&bb, "order", "(Ljava/nio/ByteOrder;)Ljava/nio/ByteBuffer;", &[JValue::Object(&order)])?.l()?)
}

/// `direct` seen as a typed view (`asIntBuffer` and friends).
fn view(c: &mut Call<'_, '_>, name: &str, sig: &str) -> Result<Ret> {
    let cap = c.i32(0)?;
    let bb = direct(c.env, cap)?;
    let buf = c.env.call_method(&bb, name, sig, &[])?.l()?;
    Ok(Ret::Obj(c.env.new_global_ref(&buf)?))
}

/// Every `Keyboard.KEY_*` of LWJGL 2.9 with the prefix stripped; its key name table is this, both ways.
const KEY_NAMES: &[(i32, &str)] = &[
    (0, "NONE"), (1, "ESCAPE"), (2, "1"), (3, "2"), (4, "3"), (5, "4"),
    (6, "5"), (7, "6"), (8, "7"), (9, "8"), (10, "9"), (11, "0"),
    (12, "MINUS"), (13, "EQUALS"), (14, "BACK"), (15, "TAB"),
    (16, "Q"), (17, "W"), (18, "E"), (19, "R"), (20, "T"), (21, "Y"),
    (22, "U"), (23, "I"), (24, "O"), (25, "P"), (26, "LBRACKET"), (27, "RBRACKET"),
    (28, "RETURN"), (29, "LCONTROL"), (30, "A"), (31, "S"), (32, "D"), (33, "F"),
    (34, "G"), (35, "H"), (36, "J"), (37, "K"), (38, "L"), (39, "SEMICOLON"),
    (40, "APOSTROPHE"), (41, "GRAVE"), (42, "LSHIFT"), (43, "BACKSLASH"),
    (44, "Z"), (45, "X"), (46, "C"), (47, "V"), (48, "B"), (49, "N"),
    (50, "M"), (51, "COMMA"), (52, "PERIOD"), (53, "SLASH"), (54, "RSHIFT"), (55, "MULTIPLY"),
    (56, "LMENU"), (57, "SPACE"), (58, "CAPITAL"), (59, "F1"), (60, "F2"), (61, "F3"),
    (62, "F4"), (63, "F5"), (64, "F6"), (65, "F7"), (66, "F8"), (67, "F9"),
    (68, "F10"), (69, "NUMLOCK"), (70, "SCROLL"), (71, "NUMPAD7"), (72, "NUMPAD8"), (73, "NUMPAD9"),
    (74, "SUBTRACT"), (75, "NUMPAD4"), (76, "NUMPAD5"), (77, "NUMPAD6"), (78, "ADD"), (79, "NUMPAD1"),
    (80, "NUMPAD2"), (81, "NUMPAD3"), (82, "NUMPAD0"), (83, "DECIMAL"), (87, "F11"), (88, "F12"),
    (100, "F13"), (101, "F14"), (102, "F15"), (103, "F16"), (104, "F17"), (105, "F18"),
    (112, "KANA"), (113, "F19"), (121, "CONVERT"), (123, "NOCONVERT"), (125, "YEN"),
    (141, "NUMPADEQUALS"), (144, "CIRCUMFLEX"), (145, "AT"), (146, "COLON"), (147, "UNDERLINE"),
    (148, "KANJI"), (149, "STOP"), (150, "AX"), (151, "UNLABELED"), (156, "NUMPADENTER"),
    (157, "RCONTROL"), (167, "SECTION"), (179, "NUMPADCOMMA"), (181, "DIVIDE"), (183, "SYSRQ"),
    (184, "RMENU"), (197, "PAUSE"), (199, "HOME"), (200, "UP"), (201, "PRIOR"), (203, "LEFT"),
    (205, "RIGHT"), (207, "END"), (208, "DOWN"), (209, "NEXT"), (210, "INSERT"), (211, "DELETE"),
    (218, "CLEAR"), (219, "LMETA"), (220, "RMETA"), (221, "APPS"), (222, "POWER"), (223, "SLEEP"),
];

/// `Keyboard.getKeyName`: null for a code LWJGL has no constant for.
fn key_name(code: i32) -> Option<&'static str> { KEY_NAMES.iter().find(|(c, _)| *c == code).map(|(_, n)| *n) }

/// `Keyboard.getKeyIndex`: anything unreachable by name is `KEY_NONE`.
fn key_index(name: &str) -> i32 { KEY_NAMES.iter().find(|(_, n)| *n == name).map_or(0, |(c, _)| *c) }

#[cfg(test)]
mod tests {
    use super::{key_index, key_name, KEY_NAMES};

    #[test]
    fn key_names_are_unique_and_round_trip() {
        for (i, (code, name)) in KEY_NAMES.iter().enumerate() {
            assert_eq!(key_name(*code), Some(*name));
            assert_eq!(key_index(name), *code, "{name}");
            assert!(!KEY_NAMES[..i].iter().any(|(c, n)| c == code || n == name), "{name}");
        }
        assert!((KEY_NAMES.len() as i32) <= 256);
    }

    #[test]
    fn unknown_keys_fall_back_to_none() {
        assert_eq!(key_name(1), Some("ESCAPE"));
        assert_eq!(key_name(56), Some("LMENU"));
        assert_eq!(key_name(84), None);
        assert_eq!(key_name(256), None);
        assert_eq!(key_index(""), 0);
        assert_eq!(key_index("BOGUS"), 0);
        assert_eq!(key_index("SPACE"), 57);
    }
}
