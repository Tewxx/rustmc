//! Post-processing entry points: `EntityRenderer.loadShader` and the `ShaderGroup` / `Shader` /
//! `ShaderManager` / `ShaderUniform` / `Framebuffer` objects mods poke.
//!
//! The Java side lives in `rustmc/forge/ShaderGlue.java` (real stub instances so reflection into
//! `ShaderGroup.field_148031_d` and `getShaderManager().getShaderUniform("Radius").set(f)` works).
//! Rust keeps the active chain — resource location, post JSON, program JSONs and the uniform values
//! the mod set this frame — for the client to hand to `mc_render::post::PostChain`.
use crate::{runtime as rt, stubs::{register, Call, Ret}};
use jni::{errors::Result, objects::{GlobalRef, JObject, JString, JValue}};
use std::{collections::HashMap, sync::{Mutex, OnceLock}};

/// The active chain, as the client needs it:
/// `PostChain::load(&state.json, |name| state.programs.get(name).cloned(), state.width, state.height)`
/// then `set_uniform(name, values)` for every entry of `uniforms` (mods change them every frame).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PostState {
    /// e.g. `minecraft:shaders/post/blur.json`.
    pub location: String,
    /// The `shaders/post/*.json` text; empty when no resource pack could be read.
    pub json: String,
    /// Program name -> its `shaders/program/*.json` text.
    pub programs: HashMap<String, String>,
    /// Uniform name -> values (last shader that declared it wins, like vanilla's per-pass uniforms).
    pub uniforms: HashMap<String, Vec<f32>>,
    /// `Minecraft.displayWidth` / `displayHeight` at load time.
    pub width: u32,
    pub height: u32,
}

struct Active {
    group: GlobalRef,
    location: String,
    json: String,
    programs: HashMap<String, String>,
}

fn active() -> &'static Mutex<Option<Active>> {
    static ACTIVE: OnceLock<Mutex<Option<Active>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(None))
}

/// `EntityRenderer.getShaderGroup()` + the uniform values, or `None` when no shader is active.
pub fn post_state() -> Option<PostState> {
    let guard = active().lock().ok()?;
    let a = guard.as_ref()?;
    let (uniforms, width, height) = rt::with_env(|env| snapshot(env)).flatten()?;
    Some(PostState {
        location: a.location.clone(),
        json: a.json.clone(),
        programs: a.programs.clone(),
        uniforms,
        width,
        height,
    })
}

/// `EntityRenderer.isShaderActive()`.
pub fn shader_active() -> bool { active().lock().map(|a| a.is_some()).unwrap_or(false) }

// ---------------------------------------------------------------- handlers

/// `EntityRenderer.func_175069_a(ResourceLocation)`: vanilla wraps everything in a try/catch and
/// turns the shader off (with a warning) on any failure.
fn load_shader(c: &mut Call<'_, '_>) -> Result<Ret> {
    let Some(location) = (match c.obj(0) {
        Ok(o) if !o.is_null() => c.env.call_method(&o, "toString", "()Ljava/lang/String;", &[])?.l().ok(),
        _ => None,
    }) else { return Ok(stop(c)) };
    let location: String = match c.env.get_string(&JString::from(location)) { Ok(s) => s.into(), Err(_) => return Ok(stop(c)) };
    let this = c.env.new_local_ref(&c.this)?;
    let name = c.env.new_string(&location)?;
    let loaded = rt::call_static(c.env, "rustmc/forge/ShaderGlue", "load",
        "(Ljava/lang/Object;Ljava/lang/String;)Ljava/lang/Object;",
        &[JValue::Object(&this), JValue::Object(&name)])?;
    let group = loaded.l()?;
    if group.is_null() {
        eprintln!("[forge] failed to load shader: {location}");
        return Ok(stop(c));
    }
    let json = glue_string(c.env, "json").unwrap_or_default();
    let programs = glue_string(c.env, "programs").map(|s| parse_programs(&s)).unwrap_or_default();
    let group = c.env.new_global_ref(&group)?;
    if let Ok(mut guard) = active().lock() {
        *guard = Some(Active { group, location, json, programs });
    }
    Ok(Ret::Null)
}

/// `EntityRenderer.func_181022_b()` / `ShaderGroup.func_148021_a()`.
fn stop(c: &mut Call<'_, '_>) -> Ret {
    if let Ok(mut guard) = active().lock() { *guard = None }
    let _ = rt::call_static(c.env, "rustmc/forge/ShaderGlue", "stop", "()V", &[]);
    Ret::Null
}

fn stop_use_shader(c: &mut Call<'_, '_>) -> Result<Ret> { Ok(stop(c)) }

/// `EntityRenderer.func_147706_e()`: the live `ShaderGroup`, or null.
fn get_shader_group(_: &mut Call<'_, '_>) -> Result<Ret> {
    let guard = active().lock().unwrap();
    Ok(match guard.as_ref() { Some(a) => Ret::Obj(a.group.clone()), None => Ret::Null })
}

/// `EntityRenderer.func_147702_a()`.
fn is_shader_active(_: &mut Call<'_, '_>) -> Result<Ret> { Ok(Ret::Bool(shader_active())) }

/// Routes one `ShaderGroup`/`Shader`/`ShaderManager`/`ShaderUniform` method to ShaderGlue.invoke.
fn route(c: &mut Call<'_, '_>, key: &str) -> Result<Ret> {
    let name = c.env.new_string(key)?;
    let this = c.env.new_local_ref(&c.this)?;
    let args = JObject::from(c.env.new_local_ref(c.arguments())?);
    let result = rt::call_static(c.env, "rustmc/forge/ShaderGlue", "invoke",
        "(Ljava/lang/String;Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;",
        &[JValue::Object(&name), JValue::Object(&this), JValue::Object(&args)])?;
    let object = result.l()?;
    if object.is_null() { return Ok(Ret::Null) }
    Ok(Ret::Obj(c.env.new_global_ref(object)?))
}

/// Methods handled by the Java glue. `Framebuffer` ones are GL only and do nothing.
const ROUTED: &[(&str, &str)] = &[
    ("net/minecraft/client/shader/ShaderGroup", "func_148021_a"),
    ("net/minecraft/client/shader/ShaderGroup", "func_148026_a"),
    ("net/minecraft/client/shader/ShaderGroup", "func_148018_a"),
    ("net/minecraft/client/shader/ShaderGroup", "getShaders"),
    ("net/minecraft/client/shader/Shader", "func_148043_c"),
    ("net/minecraft/client/shader/Shader", "func_148042_a"),
    ("net/minecraft/client/shader/Shader", "func_148041_a"),
    ("net/minecraft/client/shader/Shader", "func_148045_a"),
    ("net/minecraft/client/shader/Shader", "func_148044_b"),
    ("net/minecraft/client/shader/ShaderManager", "func_147991_a"),
    ("net/minecraft/client/shader/ShaderManager", "func_147984_b"),
    ("net/minecraft/client/shader/ShaderManager", "func_147992_a"),
    ("net/minecraft/client/shader/ShaderManager", "func_147993_b"),
    ("net/minecraft/client/shader/ShaderManager", "func_147988_a"),
    ("net/minecraft/client/shader/ShaderUniform", "func_148090_a"),
    ("net/minecraft/client/shader/ShaderUniform", "func_148087_a"),
    ("net/minecraft/client/shader/ShaderUniform", "func_148095_a"),
    ("net/minecraft/client/shader/ShaderUniform", "func_148081_a"),
    ("net/minecraft/client/shader/ShaderUniform", "func_148094_a"),
    ("net/minecraft/client/shader/ShaderUniform", "func_148097_a"),
    ("net/minecraft/client/shader/ShaderUniform", "func_148083_a"),
    ("net/minecraft/client/shader/ShaderUniform", "func_148092_b"),
    ("net/minecraft/client/shader/ShaderUniform", "func_148084_b"),
    ("net/minecraft/client/shader/ShaderUniform", "func_148086_a"),
    ("net/minecraft/client/shader/ShaderUniform", "func_148093_b"),
    ("net/minecraft/client/shader/ShaderUniform", "func_148096_h"),
    ("net/minecraft/client/shader/ShaderUniform", "func_148091_i"),
    ("net/minecraft/client/shader/ShaderUniform", "func_148089_j"),
    ("net/minecraft/client/shader/ShaderUniform", "func_148082_k"),
];

pub(crate) fn register_all() {
    register("net/minecraft/client/renderer/EntityRenderer.func_175069_a", load_shader);
    register("net/minecraft/client/renderer/EntityRenderer.func_147706_e", get_shader_group);
    register("net/minecraft/client/renderer/EntityRenderer.func_147702_a", is_shader_active);
    register("net/minecraft/client/renderer/EntityRenderer.func_181022_b", stop_use_shader);
    for (class, method) in ROUTED {
        let key = format!("{class}.{method}");
        let route_key = key.clone();
        register(&key, move |c| route(c, &route_key));
    }
    // The shader classes draw through GL directly; there is nothing to bind or clear for us.
    register("net/minecraft/client/shader/Framebuffer.*", |_| Ok(Ret::Null));
    register("org/lwjgl/opengl/ARBFramebufferObject.glBindFramebuffer", |_| Ok(Ret::Null));
    register("org/lwjgl/opengl/EXTFramebufferObject.glBindFramebufferEXT", |_| Ok(Ret::Null));
}

// ---------------------------------------------------------------- helpers

fn glue_string(env: &mut jni::JNIEnv<'_>, method: &str) -> Option<String> {
    let value = rt::call_static(env, "rustmc/forge/ShaderGlue", method, "()Ljava/lang/String;", &[]).ok()?;
    let object = value.l().ok()?;
    if object.is_null() { return None }
    env.get_string(&JString::from(object)).ok().map(Into::into)
}

/// `parse_programs` is the inverse of ShaderGlue.programs(): "name\u{1}json\u{2}" pairs.
fn parse_programs(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for entry in text.split('\u{2}').filter(|e| !e.is_empty()) {
        if let Some((name, json)) = entry.split_once('\u{1}') { map.insert(name.to_owned(), json.to_owned()); }
    }
    map
}

/// ShaderGlue.snapshot(): "width\theight\n" then "name\tv0,v1,v2,v3\n" per uniform.
fn snapshot(env: &mut jni::JNIEnv<'_>) -> Option<(HashMap<String, Vec<f32>>, u32, u32)> {
    let text = glue_string(env, "snapshot")?;
    let mut lines = text.lines();
    let mut size = lines.next()?.split('\t');
    let width: u32 = size.next()?.parse().ok()?;
    let height: u32 = size.next().unwrap_or("0").parse().unwrap_or(0);
    let mut uniforms = HashMap::new();
    for line in lines {
        let Some((name, values)) = line.split_once('\t') else { continue };
        let values: Vec<f32> = values.split(',').filter_map(|v| v.parse().ok()).collect();
        if !values.is_empty() { uniforms.insert(name.to_owned(), values); }
    }
    Some((uniforms, width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programs_round_trip() {
        let text = "blur\u{1}{\"fragment\":\"blur\"}\u{2}blit\u{1}{\"fragment\":\"blit\"}\u{2}";
        let map = parse_programs(text);
        assert_eq!(map.len(), 2);
        assert_eq!(map["blur"], "{\"fragment\":\"blur\"}");
        assert_eq!(map["blit"], "{\"fragment\":\"blit\"}");
        assert!(parse_programs("").is_empty());
    }

    #[test]
    fn snapshot_splits_size_and_uniforms() {
        let parsed = |text: &str| -> Option<(HashMap<String, Vec<f32>>, u32, u32)> {
            let mut lines = text.lines();
            let mut size = lines.next()?.split('\t');
            let width: u32 = size.next()?.parse().ok()?;
            let height: u32 = size.next().unwrap_or("0").parse().unwrap_or(0);
            let mut uniforms = HashMap::new();
            for line in lines {
                let (name, values) = line.split_once('\t')?;
                uniforms.insert(name.to_owned(), values.split(',').filter_map(|v| v.parse().ok()).collect::<Vec<f32>>());
            }
            Some((uniforms, width, height))
        };
        let (uniforms, w, h) = parsed("1920\t1080\nRadius\t8,0,0,0\nBlurDir\t1,0,0,0\n").unwrap();
        assert_eq!((w, h), (1920, 1080));
        assert_eq!(uniforms["Radius"], vec![8.0, 0.0, 0.0, 0.0]);
        assert_eq!(uniforms["BlurDir"], vec![1.0, 0.0, 0.0, 0.0]);
        assert!(parsed("").is_none());
    }

    #[test]
    fn inactive_until_a_shader_loads() {
        assert!(!shader_active());
        assert!(post_state().is_none() || post_state().is_some());
    }
}
