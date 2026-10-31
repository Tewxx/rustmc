//! Rust implementations of stubbed Java methods. To add one, `register` its SRG key in the matching file
//! (`tools/shadow/stubkey.py` prints keys); classes listed in `tools/shadow/keep-real.txt` run vanilla Java instead.
use crate::runtime as rt;
use jni::{errors::Result, objects::{GlobalRef, JObject}, JNIEnv};
use std::sync::OnceLock;

mod client;
mod gap_fml;
mod gap_models;
mod gap_gl;
mod gap_client;
mod gap_world;
mod gap_entity;
mod gap_hud;
mod entity;
mod forge;
mod gfx;
mod item;
mod lwjgl;
mod math;
mod minecraft;
mod misc;
mod render;
mod shader;
mod sound;
mod tab;
mod texture;
mod world;

pub use lwjgl::{key_bindings, release_keys, set_clipboard, set_key, set_modifiers, set_mouse, set_mouse_button, unpress_key_bindings};
pub use gfx::{begin_world, end_world, set_text_width, take_draw, Draw, Vertex};
pub use minecraft::{minecraft, set_display, set_session};
pub use misc::{set_resources, set_translations};
pub use texture::{take_texture_uploads, Upload};
pub use client::{execute_client_command, finish_ping, PingResult, is_chat_open, set_chat_open, take_client_actions, ClientAction};
pub use minecraft::{set_fps, set_fullscreen};
pub use shader::{post_state, PostState};
pub use sound::{take_sounds, SoundRequest};
pub(crate) use minecraft::{display as minecraft_display, gui_size as minecraft_gui_size};
pub(crate) use lwjgl::{set_event, MouseEvent};

pub fn register_all() {
    math::register_all();
    gap_fml::register_all();
    gap_models::register_all();
    gap_gl::register_all();
    gap_client::register_all();
    gap_world::register_all();
    gap_entity::register_all();
    gap_hud::register_all();
    minecraft::register_all();
    gfx::register_all();
    lwjgl::register_all();
    forge::register_all();
    misc::register_all();
    tab::register_all();
    texture::register_all();
    entity::register_all();
    item::register_all();
    world::register_all();
    client::register_all();
    render::register_all();
    shader::register_all();
    sound::register_all();
    crate::live::register_all();
    crate::hooks::register_all();
}

/// One lazily allocated instance of `class` (constructor not run) shared by the whole process.
fn singleton<'l>(env: &mut JNIEnv<'l>, cell: &'static OnceLock<GlobalRef>, class: &str) -> Result<JObject<'l>> {
    if cell.get().is_none() {
        let obj = rt::alloc(env, class)?;
        let _ = cell.set(env.new_global_ref(obj)?);
    }
    env.new_local_ref(cell.get().unwrap().as_obj())
}
