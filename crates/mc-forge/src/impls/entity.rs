//! Entity / EntityLivingBase / EntityPlayer(SP) and the renderer classes mods read: vanilla behaviour over the
//! stub fields `Live.sync` mirrors, answered by `rustmc.forge.Entities` (keys it does not know come back as a
//! sentinel, so never register a key there that this file's Java side does not implement).
use crate::{runtime as rt, stubs::{register, Ret}};
use jni::{errors::Result, objects::{JObject, JValue}};

pub(crate) fn register_all() {
    // Declaring class first: a stub's key is the class it is declared in, not the receiver's. The wildcard
    // also catches the overrides the stub generator emits for subclasses (`EntityPlayerSP.isBlocking`).
    for (class, methods) in [
        ("net/minecraft/entity/Entity", &[
            "func_70115_ae", "func_180425_c", "func_85030_a", "func_174818_b", "func_70057_ab", "func_145748_c_",
        ][..]),
        ("net/minecraft/entity/EntityLivingBase", &[
            "func_70651_bq", "func_70660_b", "func_110139_bj", "func_70658_aO", "func_70678_g", "func_70608_bn",
            "func_145748_c_", "func_180799_ab", "func_70631_g_",
        ]),
        ("net/minecraft/entity/player/EntityPlayer", &[
            "<init>", "func_82169_q", "func_70632_aY", "func_110139_bj", "func_174818_b", "func_70057_ab",
            "func_70115_ae", "func_70660_b", "func_110144_aD", "func_142013_aG", "func_145748_c_", "func_70631_g_",
            "func_98034_c", "func_71011_bu", "func_71052_bv", "func_71057_bx", "func_70651_bq", "func_70658_aO",
            "func_70678_g", "func_71024_bL", "func_180425_c", "func_85030_a", "func_110148_a",
        ]),
        ("net/minecraft/client/entity/AbstractClientPlayer", &[
            "<init>", "func_70632_aY", "func_175154_l", "func_70678_g", "func_71052_bv", "func_110306_p",
            "func_110303_q", "func_175148_a", "func_180799_ab", "func_70115_ae", "func_98034_c", "func_175149_v",
        ]),
        ("net/minecraft/client/entity/EntityPlayerSP", &[
            "func_70115_ae", "func_70632_aY", "func_71011_bu", "func_71057_bx", "func_180425_c", "func_110306_p",
            "func_175148_a", "func_110139_bj", "func_180799_ab", "func_70651_bq", "func_85030_a", "func_110148_a",
            "func_110303_q", "func_175149_v", "func_175154_l", "func_70658_aO", "func_70660_b", "func_70678_g",
            "func_71024_bL", "func_71052_bv", "func_98034_c",
        ]),
        ("net/minecraft/client/entity/EntityOtherPlayerMP", &[
            "<init>", "func_70632_aY", "func_175154_l", "func_110306_p", "func_70678_g", "func_71052_bv",
            "func_110139_bj", "func_175149_v",
        ]),
        ("net/minecraft/client/renderer/entity/RenderItem", &[
            "func_180450_b", "func_180453_a", "func_175050_a",
        ]),
        ("net/minecraft/client/renderer/entity/RenderManager", &[
            "func_147940_a", "func_178631_a", "func_178633_a", "getSkinMap",
        ]),
        ("net/minecraft/client/renderer/entity/RenderPlayer", &["func_177087_b"]),
        ("net/minecraft/entity/passive/EntityHorse", &["func_110148_a", "func_110261_ca"]),
        ("net/minecraft/entity/item/EntityItem", &["func_92059_d"]),
        ("net/minecraft/entity/player/InventoryPlayer", &["func_70445_o"]),
        ("net/minecraft/entity/effect/EntityLightningBolt", &["<init>"]),
    ] {
        for &method in methods {
            let key = format!("{class}.{method}");
            let route = key.clone();
            register(&key, move |c| invoke(c, &route));
            register(&format!("*.{method}"), move |c| invoke(c, &key));
        }
    }
    // The brand FML announces for a Forge client; `AbstractClientPlayer.clientBrand` comes in too late to read.
    for method in ["func_142021_k"] {
        let key = format!("net/minecraft/client/entity/AbstractClientPlayer.{method}");
        let route = key.clone();
        register(&key, move |c| brand(c, &route));
        register(&format!("*.{method}"), move |c| brand(c, &key));
    }
}

fn brand(c: &mut crate::stubs::Call<'_, '_>, _key: &str) -> Result<Ret> {
    let s = c.env.new_string("fml,forge")?;
    Ok(Ret::Obj(c.env.new_global_ref(JObject::from(s))?))
}

fn invoke(c: &mut crate::stubs::Call<'_, '_>, key: &str) -> Result<Ret> {
    let name = c.env.new_string(key)?;
    let this = c.env.new_local_ref(&c.this)?;
    let args = JObject::from(c.env.new_local_ref(c.arguments())?);
    let result = rt::call_static(c.env, "rustmc/forge/Entities", "invoke", "(Ljava/lang/String;Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;",
        &[JValue::Object(&name), JValue::Object(&this), JValue::Object(&args)])?.l()?;
    Ok(Ret::Obj(c.env.new_global_ref(result)?))
}
