//! Live Java object graph and queued mod actions. Rust remains the owner of simulation.
use crate::{runtime as rt, stubs::{register, Ret}};
use jni::{errors::Result, objects::{JClass, JObject, JValue}, JNIEnv};
use serde_json::Value;

pub fn publish(env: &mut JNIEnv<'_>, snapshot: &Value) -> Result<()> {
    let text = env.new_string(snapshot.to_string())?;
    rt::call_static(env, "rustmc/forge/Live", "sync", "(Ljava/lang/String;)V", &[JValue::Object(&text)]).map(drop)
}

pub fn changes(env: &mut JNIEnv<'_>) -> Result<Value> {
    let json = rt::call_static(env, "rustmc/forge/Live", "changes", "()Ljava/lang/String;", &[])?.l()?;
    let text: String = env.get_string(&jni::objects::JString::from(json))?.into();
    serde_json::from_str(&text).map_err(|_| jni::errors::Error::JavaException)
}

pub fn section(env: &mut JNIEnv<'_>, x: i32, y: i32, z: i32, blocks: &[u16]) -> Result<()> {
    let array = env.new_short_array(blocks.len() as i32)?;
    env.set_short_array_region(&array, 0, &blocks.iter().map(|v| *v as i16).collect::<Vec<_>>())?;
    rt::call_static(env, "rustmc/forge/Live", "section", "(III[S)V", &[JValue::Int(x), JValue::Int(y), JValue::Int(z), JValue::Object(&array)]).map(drop)
}

/// A section's light packed `sky<<4|block`; empty slices drop it.
pub fn light(env: &mut JNIEnv<'_>, x: i32, y: i32, z: i32, sky: &[u8], block: &[u8]) -> Result<()> {
    let packed: Vec<u8> = sky.iter().zip(block).map(|(s, b)| (s << 4) | (b & 15)).collect();
    let array = env.byte_array_from_slice(&packed)?;
    rt::call_static(env, "rustmc/forge/Live", "light", "(III[B)V", &[JValue::Int(x), JValue::Int(y), JValue::Int(z), JValue::Object(&array)]).map(drop)
}

/// `World.skylightSubtracted`, read by getLight / getLightBrightness.
pub fn skylight_subtracted(env: &mut JNIEnv<'_>, v: i32) -> Result<()> {
    rt::call_static(env, "rustmc/forge/Live", "skylightSubtracted", "(I)V", &[JValue::Int(v)]).map(drop)
}

pub fn biomes(env: &mut JNIEnv<'_>, x: i32, z: i32, ids: &[u8]) -> Result<()> {
    let array = env.byte_array_from_slice(ids)?;
    rt::call_static(env, "rustmc/forge/Live", "biomes", "(II[B)V", &[JValue::Int(x), JValue::Int(z), JValue::Object(&array)]).map(drop)
}

pub(crate) fn register_all() {
    register("rustmc.allocate", |c| {
        let class = JClass::from(c.obj(0)?);
        let object = c.env.alloc_object(class)?;
        Ok(Ret::Obj(c.env.new_global_ref(object)?))
    });
    for (class, methods) in [
        ("net/minecraft/entity/Entity", &["func_145782_y", "func_70005_c_", "func_70068_e", "func_70032_d", "func_70092_e", "func_70011_f", "func_174813_aQ", "func_70093_af", "func_70051_ag", "func_82150_aj", "func_70090_H", "func_70095_a", "func_70031_b", "func_82142_c", "func_70107_b", "func_70080_a", "func_174791_d", "func_174824_e", "func_70047_e", "func_70089_S", "func_70020_e", "func_110124_au", "func_70096_w", "func_70046_E", "func_70067_L"] as &[&str]),
        ("net/minecraft/entity/EntityLivingBase", &["func_110143_aJ", "func_70606_j", "func_110138_aP", "func_70694_bm", "func_71038_i", "func_70690_d", "func_70644_a"]),
        ("net/minecraft/entity/player/EntityPlayer", &["func_70005_c_", "func_146103_bH", "func_71045_bC", "func_71038_i"]),
        ("net/minecraft/client/entity/EntityPlayerSP", &["func_71165_d", "func_145747_a"]),
        ("net/minecraft/entity/player/EntityPlayer", &["func_146105_b"]),
        ("net/minecraft/entity/player/InventoryPlayer", &["func_70448_g", "func_70301_a", "func_70299_a", "func_70302_i_"]),
        ("net/minecraft/world/World", &["func_73045_a", "func_82737_E", "func_72820_D", "func_180495_p", "func_175623_d", "func_152378_a", "func_72924_a", "func_180494_b", "func_175642_b", "func_175705_a", "func_175626_b", "func_175699_k", "func_175721_c", "func_175671_l", "func_175724_o", "func_175657_ab", "func_175678_i"]),
        ("net/minecraft/client/multiplayer/WorldClient", &["func_73045_a", "func_180495_p"]),
        ("net/minecraft/client/network/NetHandlerPlayClient", &["func_147297_a"]),
        ("net/minecraft/network/NetworkManager", &["func_179290_a"]),
        ("net/minecraft/client/multiplayer/PlayerControllerMP", &["func_78764_a", "func_78750_j", "func_78757_d"]),
    ] {
        for &method in methods {
            let key = format!("{class}.{method}");
            let route = key.clone();
            register(&key, move |c| invoke(c, &route));
            // SRG method names identify the same virtual method across subclasses.
            // Generated overrides (e.g. EntityPlayerSP.setSprinting) must use it too.
            register(&format!("*.{method}"), move |c| invoke(c, &key));
        }
    }
}

fn invoke(c: &mut crate::stubs::Call<'_, '_>, key: &str) -> Result<Ret> {
    let name = c.env.new_string(key)?;
    let this = c.env.new_local_ref(&c.this)?;
    let args = JObject::from(c.env.new_local_ref(c.arguments())?);
    let result = rt::call_static(c.env, "rustmc/forge/Live", "invoke", "(Ljava/lang/String;Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;",
        &[JValue::Object(&name), JValue::Object(&this), JValue::Object(&args)])?.l()?;
    Ok(Ret::Obj(c.env.new_global_ref(result)?))
}
