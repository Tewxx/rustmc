//! LWJGL input state and `ScaledResolution`, fed by the client (`set_mouse`) and the display size.
use super::minecraft::{framebuffer_size, gui_size};
use crate::stubs::{register, Ret};
use std::sync::Mutex;

type ClipGet = Box<dyn Fn() -> String + Send>;
type ClipSet = Box<dyn Fn(&str) + Send>;
static CLIPBOARD: Mutex<Option<(ClipGet, ClipSet)>> = Mutex::new(None);

/// The system clipboard for `GuiScreen.get/setClipboardString` (vanilla uses AWT, which is off: see `runtime`).
pub fn set_clipboard(get: impl Fn() -> String + Send + 'static, set: impl Fn(&str) + Send + 'static) {
    *CLIPBOARD.lock().unwrap() = Some((Box::new(get), Box::new(set)));
}

static MOUSE: Mutex<(i32, i32)> = Mutex::new((0, 0));
static MODS: Mutex<[bool; 3]> = Mutex::new([false; 3]);

/// The current `Mouse.getEvent*` event plus held buttons, set by `game::mouse_event`.
#[derive(Clone, Copy, Default)]
pub(crate) struct MouseEvent { pub x: i32, pub y: i32, pub button: i32, pub state: bool, pub wheel: i32 }
static EVENT: Mutex<MouseEvent> = Mutex::new(MouseEvent { x: 0, y: 0, button: -1, state: false, wheel: 0 });
static HELD: Mutex<[bool; 3]> = Mutex::new([false; 3]);

/// Record one mouse event (framebuffer pixels, y from the bottom) before calling `handleMouseInput`.
pub(crate) fn set_event(e: MouseEvent) {
    if (0..3).contains(&e.button) { HELD.lock().unwrap()[e.button as usize] = e.state; }
    *EVENT.lock().unwrap() = e;
}

/// Ctrl, Shift and Alt as `Keyboard.isKeyDown` reports them (GuiScreen's ctrl+C/V/A checks read these).
pub fn set_modifiers(ctrl: bool, shift: bool, alt: bool) { *MODS.lock().unwrap() = [ctrl, shift, alt]; }

/// Pointer position in framebuffer pixels, y measured from the top (the client's window coordinates).
pub fn set_mouse(x: i32, y: i32) { *MOUSE.lock().unwrap() = (x, y); }

fn open_url(url: &str) -> std::io::Result<std::process::Child> {
    use std::process::Command;
    if cfg!(target_os = "macos") { Command::new("open").arg(url).spawn() }
    else if cfg!(target_os = "windows") { Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn() }
    else { Command::new("xdg-open").arg(url).spawn() }
}

pub fn register_all() {
    register("org/lwjgl/input/Mouse.getX", |_| Ok(MOUSE.lock().unwrap().0.into()));
    // LWJGL's origin is the bottom-left corner.
    register("org/lwjgl/input/Mouse.getY", |_| Ok((framebuffer_size().1 - 1 - MOUSE.lock().unwrap().1).into()));
    register("org/lwjgl/input/Keyboard.isKeyDown", |c| {
        let m = *MODS.lock().unwrap();
        Ok(match c.i32(0)? { 29 | 157 => m[0], 42 | 54 => m[1], 56 | 184 => m[2], _ => false }.into())
    });
    register("org/lwjgl/input/Mouse.getEventX", |_| Ok(EVENT.lock().unwrap().x.into()));
    register("org/lwjgl/input/Mouse.getEventY", |_| Ok(EVENT.lock().unwrap().y.into()));
    register("org/lwjgl/input/Mouse.getEventButton", |_| Ok(EVENT.lock().unwrap().button.into()));
    register("org/lwjgl/input/Mouse.getEventButtonState", |_| Ok(EVENT.lock().unwrap().state.into()));
    register("org/lwjgl/input/Mouse.getEventDWheel", |_| Ok(EVENT.lock().unwrap().wheel.into()));
    register("org/lwjgl/input/Mouse.isButtonDown", |c| { let b = c.i32(0)?; Ok((0..3).contains(&b).then(|| HELD.lock().unwrap()[b as usize]).unwrap_or(false).into()) });
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
