//! Renderer leaves mods reach from GUIs: `RenderItem`'s item icon / stack overlay, and the plain state of the
//! model and entity renderer classes, kept in `rustmc.forge.Render`. Icon geometry stays a `Draw::Item`: only the
//! client knows the atlas UVs for an item id and metadata.
use crate::{runtime as rt, stubs::{register, Call, Ret}};
use jni::{errors::Result, objects::{JObject, JValue}, JNIEnv};

const GLUE: &str = "rustmc/forge/Render";

pub fn register_all() {
    // (SRG name, whether the effect layer is drawn: renderItemAndEffectIntoGUI adds the glint pass).
    for (method, glint) in [("func_175042_a", false), ("func_180450_b", true)] {
        register(&format!("net/minecraft/client/renderer/entity/RenderItem.{method}"), move |c| icon(c, glint));
        register(&format!("*.{method}"), move |c| icon(c, glint));
    }
    register("net/minecraft/client/renderer/entity/RenderItem.func_180453_a", overlay);
    register("*.func_180453_a", overlay);
    // ItemModelMesher.isGui3d: our item icons are flat quads for every id, so never 3D.
    for key in ["net/minecraft/client/renderer/entity/RenderItem.func_175050_a", "*.func_175050_a"] {
        register(key, |_| Ok(false.into()));
    }
    // setBlurMipmap only picks the GL parameters of the next DynamicTexture upload.
    for key in ["net/minecraft/client/renderer/texture/DynamicTexture.func_174936_b", "*.func_174936_b"] {
        register(key, |_| Ok(Ret::Null));
    }

    let groups: &[(&str, &[&str])] = &[
        ("net/minecraft/client/model/ModelBase", &["<init>", "func_78085_a", "func_78084_a"]),
        ("net/minecraft/client/model/ModelBiped", &["<init>"]),
        ("net/minecraft/client/model/ModelPlayer", &["<init>"]),
        ("net/minecraft/client/renderer/entity/layers/LayerBipedArmor", &["func_177177_a"]),
        (
            "net/minecraft/client/model/ModelRenderer",
            &["<init>", "func_78784_a", "func_78785_a", "func_78789_a", "func_78790_a", "func_78786_a", "func_78787_b", "func_78792_a", "func_78793_a", "func_78794_c"],
        ),
        ("net/minecraft/client/model/ModelPlayer", &["func_178728_c"]),
        (
            "net/minecraft/client/renderer/entity/RenderManager",
            &["func_147940_a", "func_178631_a", "func_178633_a", "getSkinMap"],
        ),
        ("net/minecraft/client/renderer/entity/RenderPlayer", &["func_177087_b", "<init>"]),
        ("net/minecraft/client/renderer/entity/Render", &["<init>"]),
        ("net/minecraft/client/renderer/entity/RendererLivingEntity", &["<init>", "func_177094_a", "func_177089_b"]),
        ("net/minecraft/client/renderer/ItemRenderer", &["func_178099_a"]),
        ("net/minecraft/client/renderer/texture/TextureUtil", &["func_177053_a"]),
        ("net/minecraft/client/resources/DefaultPlayerSkin", &["func_177334_a"]),
        ("net/minecraft/client/resources/SkinManager", &["func_152790_a"]),
    ];
    for (class, methods) in groups {
        for &method in *methods {
            let key = format!("{class}.{method}");
            register(&key, { let k = key.clone(); move |c| invoke(c, &k) });
            // SRG names identify one virtual method across subclasses; mod overrides call back into these.
            if !matches!(method, "<init>" | "getSkinMap") {
                register(&format!("*.{method}"), { let k = key.clone(); move |c| invoke(c, &k) });
            }
        }
    }
}

fn invoke(c: &mut Call<'_, '_>, key: &str) -> Result<Ret> {
    let name = c.env.new_string(key)?;
    let this = c.env.new_local_ref(&c.this)?;
    let args = JObject::from(c.env.new_local_ref(c.arguments())?);
    let result = rt::call_static(c.env, GLUE, "invoke", "(Ljava/lang/String;Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;",
        &[JValue::Object(&name), JValue::Object(&this), JValue::Object(&args)])?.l()?;
    if result.is_null() {
        return Ok(Ret::Null);
    }
    Ok(Ret::Obj(c.env.new_global_ref(result)?))
}

/// `renderItemIntoGUI` / `renderItemAndEffectIntoGUI`: the icon is drawn at the current model-view transform.
fn icon(c: &mut Call<'_, '_>, effect: bool) -> Result<Ret> {
    let stack = c.obj(0)?;
    if stack.is_null() {
        return Ok(Ret::Null);
    }
    let (id, meta) = stack_item(c.env, &stack)?;
    if id == 0 {
        return Ok(Ret::Null);
    }
    let glint = effect && has_effect(c.env, &stack)?;
    push_item(id, meta, c.i32(1)? as f32, c.i32(2)? as f32, glint);
    Ok(Ret::Null)
}

/// `Item.getIdFromItem(stack.getItem())` and `stack.getItemDamage()`.
fn stack_item(env: &mut JNIEnv<'_>, stack: &JObject<'_>) -> Result<(u16, u16)> {
    let meta = env.get_field(stack, "field_77991_e", "I")?.i()? as u16;
    let item = env.get_field(stack, "field_151002_e", "Lnet/minecraft/item/Item;")?.l()?;
    if item.is_null() {
        return Ok((0, meta));
    }
    let id = rt::call_static(env, "net/minecraft/item/Item", "func_150891_b", "(Lnet/minecraft/item/Item;)I", &[JValue::Object(&item)])?.i()?;
    Ok((id as u16, meta))
}

fn has_effect(env: &mut JNIEnv<'_>, stack: &JObject<'_>) -> Result<bool> {
    Ok(env.call_method(stack, "func_77962_s", "()Z", &[])?.z()?)
}

/// `renderItemOverlayIntoGUI`: the stack count in the lower right corner, then the damage bar under the icon.
fn overlay(c: &mut Call<'_, '_>) -> Result<Ret> {
    let stack = c.obj(1)?;
    if stack.is_null() {
        return Ok(Ret::Null);
    }
    let fr = c.obj(0)?;
    let (x, y) = (c.i32(2)?, c.i32(3)?);
    let count = c.env.get_field(&stack, "field_77994_a", "I")?.i()?;
    let text = c.string(4)?;
    if count != 1 || text.is_some() {
        let s = match text {
            Some(t) => t,
            // EnumChatFormatting.RED marks an empty stack.
            None if count < 1 => format!("\u{a7}c{count}"),
            None => count.to_string(),
        };
        let ask = JObject::from(c.env.new_string(&s)?);
        let width = call_int(c.env, &fr, "func_78256_a", "(Ljava/lang/String;)I", &[JValue::Object(&ask)])?;
        let arg = JObject::from(c.env.new_string(&s)?);
        c.env.call_method(&fr, "func_175063_a", "(Ljava/lang/String;FFI)I", &[
            JValue::Object(&arg),
            JValue::Float((x + 19 - 2 - width) as f32),
            JValue::Float((y + 6 + 3) as f32),
            JValue::Int(16777215),
        ])?;
    }
    // isItemDamaged(): only the damage value decides, and 1.8.9's getMaxDamage is 0 for anything unregistered.
    let damage = c.env.get_field(&stack, "field_77991_e", "I")?.i()?;
    let max = if damage > 0 { call_int(c.env, &stack, "func_77958_k", "()I", &[])? } else { 0 };
    if damage > 0 && max > 0 {
        let j = (13.0 - damage as f64 * 13.0 / max as f64 + 0.5).floor() as i32;
        let i = (255.0 - damage as f64 * 255.0 / max as f64 + 0.5).floor() as i32;
        // The bars are untextured quads: Disable/EnableTexture2D pick the sink state they land in.
        let _ = rt::call_static(c.env, "net/minecraft/client/renderer/GlStateManager", "func_179090_x", "()V", &[]);
        for (w, h, r, g, b) in [(13, 2, 0, 0, 0), (12, 1, (255 - i) / 4, 64, 0), (j, 1, 255 - i, i, 0)] {
            rect(c.env, x + 2, y + 13, w, h, r, g, b)?;
        }
        let _ = rt::call_static(c.env, "net/minecraft/client/renderer/GlStateManager", "func_179098_w", "()V", &[]);
    }
    Ok(Ret::Null)
}

/// `RenderItem.draw`: one coloured rectangle through the Tessellator the sink already understands.
fn rect(env: &mut JNIEnv<'_>, x: i32, y: i32, w: i32, h: i32, r: i32, g: i32, b: i32) -> Result<()> {
    let tess = rt::call_static(env, "net/minecraft/client/renderer/Tessellator", "func_178181_a", "()Lnet/minecraft/client/renderer/Tessellator;", &[])?.l()?;
    let wr = env.call_method(&tess, "func_178180_c", "()Lnet/minecraft/client/renderer/WorldRenderer;", &[])?.l()?;
    let format = JObject::null();
    env.call_method(&wr, "func_181668_a", "(ILnet/minecraft/client/renderer/vertex/VertexFormat;)V", &[JValue::Int(7), JValue::Object(&format)])?;
    for (px, py) in [(x, y + h), (x + w, y + h), (x + w, y), (x, y)] {
        env.call_method(&wr, "func_181662_b", "(DDD)Lnet/minecraft/client/renderer/WorldRenderer;",
            &[JValue::Double(px as f64), JValue::Double(py as f64), JValue::Double(0.0)])?;
        env.call_method(&wr, "func_181669_b", "(IIII)Lnet/minecraft/client/renderer/WorldRenderer;",
            &[JValue::Int(r), JValue::Int(g), JValue::Int(b), JValue::Int(255)])?;
        env.call_method(&wr, "func_181675_d", "()V", &[])?;
    }
    env.call_method(&tess, "func_78381_a", "()V", &[])?;
    Ok(())
}

fn call_int(env: &mut JNIEnv<'_>, obj: &JObject<'_>, method: &str, sig: &str, args: &[JValue]) -> Result<i32> {
    Ok(env.call_method(obj, method, sig, args)?.i()?)
}

/// Hands one item icon to the draw sink. `gfx` owns the sink and needs this hook (reported, not this crate):
/// `pub(crate) fn push_item(id: u16, meta: u16, x: f32, y: f32, glint: bool)` — compose the model-view like
/// `Draw::Text` does and push `Draw::Item { id, meta, x, y, scale, glint }` with the icon-unit scale.
fn push_item(id: u16, meta: u16, x: f32, y: f32, glint: bool) {
    let _ = (id, meta, x, y, glint);
}

#[cfg(test)]
mod tests {
    /// Vanilla draws `13 - damage * 13 / max` pixels of the red bar and `255 - damage * 255 / max` of green.
    fn bars(damage: i32, max: i32) -> (i32, i32) {
        let d = damage as f64;
        let m = max as f64;
        ((13.0 - d * 13.0 / m + 0.5).floor() as i32, (255.0 - d * 255.0 / m + 0.5).floor() as i32)
    }

    #[test]
    fn damage_bar_spans_full_width_when_new() {
        assert_eq!(bars(1, 100), (13, 252));
    }

    #[test]
    fn damage_bar_shrinks_to_nothing_when_broken() {
        assert_eq!(bars(100, 100), (0, 0));
        assert_eq!(bars(50, 100), (7, 128));
    }
}
