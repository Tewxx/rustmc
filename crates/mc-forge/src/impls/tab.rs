//! The tab list and scoreboard teams: NetHandlerPlayClient / NetworkPlayerInfo / Scoreboard / ScorePlayerTeam,
//! backed by `rustmc.forge.Tab` (mirrored from the snapshot's `tab`).
use crate::{runtime as rt, stubs::{register, Ret}};
use jni::{errors::Result, objects::{JObject, JValue}};

pub(crate) fn register_all() {
    // Declaring class first: a stub's key is the class it is declared in, not the receiver's.
    for (class, methods) in [
        ("net/minecraft/client/Minecraft", &["func_147114_u"][..]),
        ("net/minecraft/client/network/NetHandlerPlayClient", &["func_175106_d", "func_175102_a", "func_175104_a"]),
        ("net/minecraft/client/network/NetworkPlayerInfo", &[
            "func_178845_a", "func_178848_b", "func_178853_c", "func_178854_k", "func_178850_i",
        ]),
        ("net/minecraft/world/World", &["func_96441_U"]),
        ("net/minecraft/scoreboard/Scoreboard", &["func_96508_e", "func_96509_i", "func_96525_g", "func_96531_f"]),
        ("net/minecraft/scoreboard/Team", &["func_142053_d", "func_96667_a", "func_142054_a", "func_96661_b"]),
        ("net/minecraft/scoreboard/ScorePlayerTeam", &[
            "func_96661_b", "func_96669_c", "func_96668_e", "func_96663_f", "func_96670_d", "func_178775_l",
        ]),
        ("net/minecraft/entity/EntityLivingBase", &["func_96124_cp", "func_142014_c"]),
        ("net/minecraft/entity/player/EntityPlayer", &["func_96124_cp"]),
        ("net/minecraft/entity/passive/EntityTameable", &["func_96124_cp"]),
    ] {
        for &method in methods {
            let key = format!("{class}.{method}");
            let route = key.clone();
            register(&key, move |c| invoke(c, &route));
            // SRG names identify the same virtual method across subclasses; generated overrides
            // (EntityPlayerSP.getTeam, ...) must land here too.
            register(&format!("*.{method}"), move |c| invoke(c, &key));
        }
    }
}

fn invoke(c: &mut crate::stubs::Call<'_, '_>, key: &str) -> Result<Ret> {
    let name = c.env.new_string(key)?;
    let this = c.env.new_local_ref(&c.this)?;
    let args = JObject::from(c.env.new_local_ref(c.arguments())?);
    let result = rt::call_static(c.env, "rustmc/forge/Tab", "invoke", "(Ljava/lang/String;Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;",
        &[JValue::Object(&name), JValue::Object(&this), JValue::Object(&args)])?.l()?;
    Ok(Ret::Obj(c.env.new_global_ref(result)?))
}
