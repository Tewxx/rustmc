//! Mod-made textures: `AbstractTexture` GL ids, `DynamicTexture` pixel uploads and `TextureManager`'s registry.
//! A texture with GL id N is the GUI texture `rustmc/gl/N`; the client uploads queued pixels before replaying draws.
use crate::{runtime as rt, stubs::{register, Ret}};
use jni::{errors::Result, objects::{GlobalRef, JIntArray, JObject, JValue}, JNIEnv};
use std::{collections::HashMap, sync::{atomic::{AtomicI32, Ordering}, Mutex}};

/// Above anything vanilla's `GL11.glGenTextures` would hand out in a real game.
static NEXT_ID: AtomicI32 = AtomicI32::new(1 << 20);
static UPLOADS: Mutex<Vec<Upload>> = Mutex::new(Vec::new());
/// `TextureManager.mapTextureObjects`: location path -> texture object, plus `getDynamicTextureLocation` counters.
static LOADED: Mutex<Option<(HashMap<String, GlobalRef>, HashMap<String, i32>)>> = Mutex::new(None);
/// Location path -> GL id, for `bindTexture(ResourceLocation)`.
static PATHS: Mutex<Option<HashMap<String, i32>>> = Mutex::new(None);
const MAX_PIXELS: i64 = 4096 * 4096;

/// One `TextureUtil.uploadTexture`: GUI texture path, size, RGBA bytes.
pub struct Upload { pub path: String, pub width: u32, pub height: u32, pub rgba: Vec<u8> }

/// Pixels mods uploaded since the last call, oldest first; a later upload of the same path replaces the earlier.
pub fn take_texture_uploads() -> Vec<Upload> { std::mem::take(&mut *UPLOADS.lock().unwrap()) }

fn gl_path(id: i32) -> String { format!("rustmc/gl/{id}") }

/// The draw texture for a bound location: its mod texture when one was loaded under it, else the asset path.
pub(crate) fn resolve(path: String) -> String {
    match PATHS.lock().unwrap().as_ref().and_then(|m| m.get(&path)) { Some(&id) => gl_path(id), None => path }
}

fn gl_id(env: &mut JNIEnv<'_>, texture: &JObject<'_>) -> Result<i32> {
    let id = env.get_field(texture, "field_110553_a", "I")?.i()?;
    if id > 0 { return Ok(id); }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    env.set_field(texture, "field_110553_a", "I", JValue::Int(id))?;
    Ok(id)
}

/// ARGB ints as RGBA bytes.
fn rgba(argb: &[i32]) -> Vec<u8> { argb.iter().flat_map(|&p| { let p = p as u32; [(p >> 16) as u8, (p >> 8) as u8, p as u8, (p >> 24) as u8] }).collect() }

fn upload(env: &mut JNIEnv<'_>, texture: &JObject<'_>) -> Result<()> {
    let (w, h) = (env.get_field(texture, "field_94233_j", "I")?.i()?, env.get_field(texture, "field_94234_k", "I")?.i()?);
    let data = JIntArray::from(env.get_field(texture, "field_110566_b", "[I")?.l()?);
    if data.is_null() || w <= 0 || h <= 0 || (w as i64) * (h as i64) > MAX_PIXELS { return Ok(()); }
    let mut pixels = vec![0i32; (w * h) as usize];
    if env.get_array_length(&data)? < w * h { return Ok(()); }
    env.get_int_array_region(&data, 0, &mut pixels)?;
    let path = gl_path(gl_id(env, texture)?);
    let mut uploads = UPLOADS.lock().unwrap();
    uploads.retain(|u| u.path != path);
    uploads.push(Upload { path, width: w as u32, height: h as u32, rgba: rgba(&pixels) });
    Ok(())
}

fn location_path(env: &mut JNIEnv<'_>, loc: &JObject<'_>) -> Result<Option<String>> {
    if loc.is_null() { return Ok(None); }
    let get = |env: &mut JNIEnv<'_>, f: &str| -> Result<String> { let s = env.get_field(loc, f, "Ljava/lang/String;")?.l()?; Ok(env.get_string(&s.into())?.into()) };
    let (domain, path) = (get(env, "field_110626_a")?, get(env, "field_110625_b")?);
    Ok(Some(if domain == "minecraft" { path } else { format!("{domain}:{path}") }))
}

fn remember(env: &mut JNIEnv<'_>, path: String, texture: &JObject<'_>) -> Result<()> {
    let id = gl_id(env, texture)?;
    let global = env.new_global_ref(texture)?;
    LOADED.lock().unwrap().get_or_insert_with(Default::default).0.insert(path.clone(), global);
    PATHS.lock().unwrap().get_or_insert_with(Default::default).insert(path, id);
    Ok(())
}

pub(crate) fn register_all() {
    const AT: &str = "net/minecraft/client/renderer/texture/AbstractTexture.";
    const DT: &str = "net/minecraft/client/renderer/texture/DynamicTexture.";
    const TM: &str = "net/minecraft/client/renderer/texture/TextureManager.";
    register(&format!("{AT}<init>"), |c| { let this = c.env.new_local_ref(&c.this)?; c.env.set_field(&this, "field_110553_a", "I", JValue::Int(-1))?; Ok(Ret::Null) });
    register(&format!("{AT}func_110552_b"), |c| { let this = c.env.new_local_ref(&c.this)?; Ok(gl_id(c.env, &this)?.into()) });
    // deleteGlTexture: the id is not reused; a later getGlTextureId allocates a fresh one.
    register(&format!("{AT}func_147631_c"), |c| { let this = c.env.new_local_ref(&c.this)?; c.env.set_field(&this, "field_110553_a", "I", JValue::Int(-1))?; Ok(Ret::Null) });
    // DynamicTexture(BufferedImage) / (width, height); the image constructor copies getRGB and uploads.
    register(&format!("{DT}<init>"), |c| {
        let this = c.env.new_local_ref(&c.this)?;
        c.env.set_field(&this, "field_110553_a", "I", JValue::Int(-1))?;
        let first = c.obj(0)?;
        let image = c.env.is_instance_of(&first, "java/awt/image/BufferedImage")?;
        let (w, h) = if image {
            (c.env.call_method(&first, "getWidth", "()I", &[])?.i()?, c.env.call_method(&first, "getHeight", "()I", &[])?.i()?)
        } else { (c.i32(0)?, c.i32(1)?) };
        let (w, h) = (w.max(0), h.max(0));
        if (w as i64) * (h as i64) > MAX_PIXELS { c.env.throw_new("java/lang/IllegalArgumentException", "texture too large")?; return Ok(Ret::Null); }
        let data = c.env.new_int_array(w * h)?;
        c.env.set_field(&this, "field_94233_j", "I", JValue::Int(w))?;
        c.env.set_field(&this, "field_94234_k", "I", JValue::Int(h))?;
        c.env.set_field(&this, "field_110566_b", "[I", JValue::Object(&data))?;
        if image {
            c.env.call_method(&first, "getRGB", "(IIII[III)[I", &[JValue::Int(0), JValue::Int(0), JValue::Int(w), JValue::Int(h), JValue::Object(&data), JValue::Int(0), JValue::Int(w)])?;
            upload(c.env, &this)?;
        }
        Ok(Ret::Null)
    });
    register(&format!("{DT}func_110564_a"), |c| { let this = c.env.new_local_ref(&c.this)?; upload(c.env, &this)?; Ok(Ret::Null) });
    register(&format!("{DT}func_110565_c"), |c| {
        let this = c.env.new_local_ref(&c.this)?;
        let data = c.env.get_field(&this, "field_110566_b", "[I")?.l()?;
        Ok(Ret::Obj(c.env.new_global_ref(data)?))
    });
    register(&format!("{DT}func_110551_a"), |_| Ok(Ret::Null)); // loadTexture: nothing to read from packs
    // getDynamicTextureLocation(name, texture): "dynamic/<name>_<n>", n counting from 1 per name.
    register(&format!("{TM}func_110578_a"), |c| {
        let name = c.string(0)?.unwrap_or_default();
        let texture = c.obj(1)?;
        let n = { let mut l = LOADED.lock().unwrap(); let n = l.get_or_insert_with(Default::default).1.entry(name.clone()).or_insert(0); *n += 1; *n };
        let path = format!("dynamic/{name}_{n}");
        let s = c.env.new_string(&path)?;
        let loc = rt::new_object(c.env, "net/minecraft/util/ResourceLocation", "(Ljava/lang/String;)V", &[JValue::Object(&s)])?;
        // Keyed by the location's own path: ResourceLocation lowercases it, and binds look it up that way.
        if !texture.is_null() { if let Some(path) = location_path(c.env, &loc)? { remember(c.env, path, &texture)?; } }
        Ok(Ret::Obj(c.env.new_global_ref(loc)?))
    });
    // loadTexture(location, texture): mod textures are already in memory; asset textures load natively.
    register(&format!("{TM}func_110579_a"), |c| {
        let (loc, texture) = (c.obj(0)?, c.obj(1)?);
        if let Some(path) = location_path(c.env, &loc)? {
            let class = rt::class(c.env, "net/minecraft/client/renderer/texture/DynamicTexture")?;
            let dynamic = !texture.is_null() && c.env.is_instance_of(&texture, class)?;
            if dynamic { remember(c.env, path, &texture)?; }
        }
        Ok(true.into())
    });
    register(&format!("{TM}func_110581_b"), |c| {
        let loc = c.obj(0)?;
        let Some(path) = location_path(c.env, &loc)? else { return Ok(Ret::Null) };
        let found = LOADED.lock().unwrap().as_ref().and_then(|l| l.0.get(&path).cloned());
        Ok(found.map_or(Ret::Null, Ret::Obj))
    });
    register(&format!("{TM}func_147645_c"), |c| {
        let loc = c.obj(0)?;
        if let Some(path) = location_path(c.env, &loc)? {
            if let Some(l) = LOADED.lock().unwrap().as_mut() { l.0.remove(&path); }
            if let Some(p) = PATHS.lock().unwrap().as_mut() { p.remove(&path); }
        }
        Ok(Ret::Null)
    });
    // GlStateManager.bindTexture(id): a mod texture's id, or vanilla state Rust does not track (untextured).
    register("net/minecraft/client/renderer/GlStateManager.func_179144_i", |c| {
        let id = c.i32(0)?;
        let known = id >= 1 << 20 && id < NEXT_ID.load(Ordering::Relaxed);
        super::gfx::bind_texture(known.then(|| gl_path(id)));
        Ok(Ret::Null)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn argb_ints_become_rgba_bytes() {
        assert_eq!(rgba(&[0x80ff_2010u32 as i32, 0x0000_00ff]), vec![0xff, 0x20, 0x10, 0x80, 0, 0, 0xff, 0]);
    }
    #[test]
    fn bound_locations_resolve_to_their_mod_texture() {
        PATHS.lock().unwrap().get_or_insert_with(Default::default).insert("dynamic/test_1".into(), 1 << 20);
        assert_eq!(resolve("dynamic/test_1".into()), format!("rustmc/gl/{}", 1 << 20));
        assert_eq!(resolve("textures/gui/widgets.png".into()), "textures/gui/widgets.png");
    }
}
