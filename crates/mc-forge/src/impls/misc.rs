//! Small vanilla leaves with no better home: I18n, ItemStack constructors, GL state that has no effect here.
use crate::{runtime as rt, stubs::{register, Ret}};
use jni::objects::JValue;
use std::{collections::HashMap, sync::RwLock};

type Reader = Box<dyn Fn(&str, bool) -> Option<Vec<u8>> + Send + Sync>;
static RESOURCES: RwLock<Option<Reader>> = RwLock::new(None);

/// The client's resource pack stack for `Minecraft.getResourceManager`: `read(full, base)` reads `assets/<ns>/<path>`
/// from the default pack when `base`, else from the packs above it. Reset whenever the client reloads resources.
pub fn set_resources(read: impl Fn(&str, bool) -> Option<Vec<u8>> + Send + Sync + 'static) {
    *RESOURCES.write().unwrap() = Some(Box::new(read));
}

static LANG: RwLock<Option<HashMap<String, String>>> = RwLock::new(None);

/// The client's current language table (`LanguageManager`), for `I18n.format`.
pub fn set_translations(map: HashMap<String, String>) { *LANG.write().unwrap() = Some(map); }

fn translate(key: &str) -> String {
    LANG.read().unwrap().as_ref().and_then(|m| m.get(key).cloned()).unwrap_or_else(|| key.to_owned())
}

pub(crate) fn register_all() {
    // Bootstrap.isRegistered: ModLoader.registries runs vanilla's registry half before any mod code.
    register("net/minecraft/init/Bootstrap.func_179869_a", |_| Ok(true.into()));
    // EntityLivingBase.getArmSwingAnimationEnd: haste shortens the swing, mining fatigue lengthens it.
    register("net/minecraft/entity/EntityLivingBase.func_82166_i", |c| {
        let mut amp = |id: i32| -> jni::errors::Result<Option<i32>> {
            let potion = c.env.get_static_field("net/minecraft/potion/Potion", "field_76425_a", "[Lnet/minecraft/potion/Potion;")?.l()?;
            let effect = c.env.get_object_array_element(&jni::objects::JObjectArray::from(potion), id)?;
            let effect = c.env.call_method(&c.this, "func_70660_b", "(Lnet/minecraft/potion/Potion;)Lnet/minecraft/potion/PotionEffect;", &[JValue::Object(&effect)])?.l()?;
            if effect.is_null() { return Ok(None); }
            Ok(Some(c.env.call_method(&effect, "func_76458_c", "()I", &[])?.i()?))
        };
        Ok(Ret::Int(if let Some(a) = amp(3)? { 6 - (1 + a) } else if let Some(a) = amp(4)? { 6 + (1 + a) * 2 } else { 6 }))
    });
    for (key, base) in [("rustmc/forge/Resources.readBase", true), ("rustmc/forge/Resources.readPacks", false)] {
        register(key, move |c| {
            let path = c.string(0)?.unwrap_or_default();
            let bytes = RESOURCES.read().unwrap().as_ref().and_then(|r| r(&path, base));
            match bytes {
                Some(b) => { let a = c.env.byte_array_from_slice(&b)?; Ok(Ret::Obj(c.env.new_global_ref(a)?)) }
                None => Ok(Ret::Null),
            }
        });
    }
    // I18n.format: String.format over the translation, "Format error: " like Locale.formatMessage.
    register("net/minecraft/client/resources/I18n.func_135052_a", |c| {
        let key = c.string(0)?.unwrap_or_default();
        let pattern = translate(&key);
        let args = c.obj(1)?;
        let format = c.env.new_string(&pattern)?;
        let out = c.env.call_static_method("java/lang/String", "format", "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;",
            &[JValue::Object(&format), JValue::Object(&args)]);
        if out.is_err() && c.env.exception_check()? {
            c.env.exception_clear()?;
            return Ok(format!("Format error: {pattern}").into());
        }
        Ok(Ret::Obj(c.env.new_global_ref(out?.l()?)?))
    });
    // ItemStack(Item|Block[, size[, damage]]): a Block becomes Item.getItemFromBlock.
    register("net/minecraft/item/ItemStack.<init>", |c| {
        let n = c.env.get_array_length(c.arguments())?;
        let mut item = c.obj(0)?;
        let block = rt::class(c.env, "net/minecraft/block/Block")?;
        if !item.is_null() && c.env.is_instance_of(&item, block)? {
            item = rt::call_static(c.env, "net/minecraft/item/Item", "func_150898_a", "(Lnet/minecraft/block/Block;)Lnet/minecraft/item/Item;", &[JValue::Object(&item)])?.l()?;
        }
        let size = if n > 1 { c.i32(1)? } else { 1 };
        let damage = if n > 2 { c.i32(2)? } else { 0 };
        let this = c.env.new_local_ref(&c.this)?;
        c.env.set_field(&this, "field_151002_e", "Lnet/minecraft/item/Item;", JValue::Object(&item))?;
        c.env.set_field(&this, "field_77994_a", "I", JValue::Int(size))?;
        c.env.set_field(&this, "field_77991_e", "I", JValue::Int(damage.max(0)))?;
        Ok(Ret::Null)
    });
    // Samplers and mipmaps belong to the native renderer.
    for key in ["org/lwjgl/opengl/GL11.glTexParameteri", "org/lwjgl/opengl/GL11.glTexParameterf", "org/lwjgl/opengl/GL30.glGenerateMipmap",
                "net/minecraft/client/gui/GuiMainMenu.<init>"] {
        register(key, |_| Ok(Ret::Null));
    }
    register("net/minecraft/entity/player/EntityPlayer.func_71039_bw", |c| {
        let this = c.env.new_local_ref(&c.this)?;
        let using = c.env.get_field(&this, "field_71074_e", "Lnet/minecraft/item/ItemStack;")?.l()?;
        Ok((!using.is_null()).into())
    });
}
