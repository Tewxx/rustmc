//! Rust implementations of stubbed Java methods. To add one, `register` its SRG key in the matching file
//! (`tools/shadow/stubkey.py` prints keys); classes listed in `tools/shadow/keep-real.txt` run vanilla Java instead.
use crate::runtime as rt;
use jni::{errors::Result, objects::{GlobalRef, JObject}, JNIEnv};
use std::sync::OnceLock;

mod forge;
mod gfx;
mod lwjgl;
mod math;
mod minecraft;

pub use lwjgl::{set_clipboard, set_modifiers, set_mouse};
pub use gfx::{set_text_width, take_draw, Draw, Vertex};
pub use minecraft::{minecraft, set_display, set_session};
pub(crate) use minecraft::{display as minecraft_display, gui_size as minecraft_gui_size};
pub(crate) use lwjgl::{set_event, MouseEvent};

pub fn register_all() {
    math::register_all();
    minecraft::register_all();
    gfx::register_all();
    lwjgl::register_all();
    forge::register_all();
}

/// One lazily allocated instance of `class` (constructor not run) shared by the whole process.
fn singleton<'l>(env: &mut JNIEnv<'l>, cell: &'static OnceLock<GlobalRef>, class: &str) -> Result<JObject<'l>> {
    if cell.get().is_none() {
        let obj = rt::alloc(env, class)?;
        let _ = cell.set(env.new_global_ref(obj)?);
    }
    env.new_local_ref(cell.get().unwrap().as_obj())
}
