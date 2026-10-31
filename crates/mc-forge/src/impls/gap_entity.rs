//! Forge API surface mods reach for entities and the local player: methods the corpus scan found unimplemented.
//! Vanilla's own answer over the fields `Live.sync` mirrors, plus `DataWatcher`/`Blocks`/`EnumFacing`, which the
//! stub jar keeps real.
use crate::{game, runtime as rt, stubs::{register, Call, Ret}};
use jni::{errors::Result, objects::{JObject, JValue}, JNIEnv};

const AABB: &str = "Lnet/minecraft/util/AxisAlignedBB;";
const BLOCK: &str = "Lnet/minecraft/block/Block;";
const BLOCK_STATE: &str = "Lnet/minecraft/block/state/IBlockState;";
const CAPABILITIES: &str = "Lnet/minecraft/entity/player/PlayerCapabilities;";
const DATA_WATCHER: &str = "Lnet/minecraft/entity/DataWatcher;";
const ITEM_STACK: &str = "Lnet/minecraft/item/ItemStack;";
const MAP: &str = "Ljava/util/Map;";
const WORLD: &str = "Lnet/minecraft/world/World;";
const GET_BLOCK_STATE: &str = "(Lnet/minecraft/util/BlockPos;)Lnet/minecraft/block/state/IBlockState;";

/// `Live.entityLight`: the entity's brightness (float) or packed combined light (int).
pub(crate) fn entity_light(c: &mut Call<'_, '_>, combined: bool, eye: bool) -> Result<Ret> {
    let this = c.env.new_local_ref(&c.this)?;
    let v = rt::call_static(c.env, "rustmc/forge/Live", "entityLight", "(Ljava/lang/Object;ZZ)Ljava/lang/Object;",
        &[JValue::Object(&this), JValue::Bool(combined as u8), JValue::Bool(eye as u8)])?.l()?;
    Ok(if combined { Ret::Int(c.env.call_method(&v, "intValue", "()I", &[])?.i()?) } else { Ret::Float(c.env.call_method(&v, "floatValue", "()F", &[])?.f()?) })
}

pub(crate) fn register_all() {
    // Mods allocate their own entities (`new EntityDummy(world)`); Entity's constructor does what every other
    // entity constructor here does: the fields Entities.construct fills for a mod-built object.
    register("net/minecraft/entity/Entity.<init>", |c| { invoke(c, "<init>")?; Ok(Ret::Null) });

    // Entity.getHorizontalFacing / getLook / hitByEntity / hashCode: inherited unchanged by every subclass.
    register("net/minecraft/entity/Entity.func_174811_aO", horizontal_facing);
    register("*.func_174811_aO", horizontal_facing);
    register("net/minecraft/entity/Entity.func_70676_i", get_look);
    register("*.func_70676_i", get_look);
    // `hitByEntity` is `return false` in 1.8.9, with no override anywhere.
    register("net/minecraft/entity/Entity.func_85031_j", |_| Ok(Ret::Bool(false)));
    register("*.func_85031_j", |_| Ok(Ret::Bool(false)));
    // Entity.hashCode is the entity id; only Entity and EntityPlayer are registered by name, since a `*.hashCode`
    // would answer every fake entity id for maps keyed by unrelated net.minecraft objects.
    for class in ["entity/Entity", "entity/player/EntityPlayer"] {
        register(&format!("net/minecraft/{class}.hashCode"), |c| {
            Ok(Ret::Int(c.env.get_field(&c.this, "field_145783_c", "I")?.i()?))
        });
    }

    register("net/minecraft/entity/item/EntityArmorStand.func_181026_s", has_marker);
    register("net/minecraft/entity/item/EntityItemFrame.func_82335_i", displayed_item);

    // EntityPlayer.getUUID(profile): the profile's id, else getOfflineUUID(name) =
    // UUID.nameUUIDFromBytes(("OfflinePlayer:" + name) in UTF-8).
    register("net/minecraft/entity/player/EntityPlayer.func_146094_a", |c| {
        let profile = c.obj(0)?;
        let id = c.env.call_method(&profile, "getId", "()Ljava/util/UUID;", &[])?.l()?;
        if !id.is_null() { return Ok(Ret::Obj(c.env.new_global_ref(id)?)); }
        let name = c.env.call_method(&profile, "getName", "()Ljava/lang/String;", &[])?.l()?;
        let name: String = c.env.get_string(&name.into())?.into();
        let bytes = c.env.byte_array_from_slice(format!("OfflinePlayer:{name}").as_bytes())?;
        let uuid = rt::call_static(c.env, "java/util/UUID", "nameUUIDFromBytes", "([B)Ljava/util/UUID;", &[JValue::Object(&bytes)])?.l()?;
        Ok(Ret::Obj(c.env.new_global_ref(uuid)?))
    });
    register("net/minecraft/entity/player/EntityPlayer.func_175147_b", |c| {
        let name = c.string(0)?.unwrap_or_default();
        let bytes = c.env.byte_array_from_slice(format!("OfflinePlayer:{name}").as_bytes())?;
        let uuid = rt::call_static(c.env, "java/util/UUID", "nameUUIDFromBytes", "([B)Ljava/util/UUID;", &[JValue::Object(&bytes)])?.l()?;
        Ok(Ret::Obj(c.env.new_global_ref(uuid)?))
    });

    // getBrightness / getBrightnessForRender over the light Live mirrors.
    for class in ["entity/Entity", "entity/EntityLivingBase", "*"] {
        let key = |m: &str| if class == "*" { format!("*.{m}") } else { format!("net/minecraft/{class}.{m}") };
        register(&key("func_70013_c"), |c| entity_light(c, false, true));
        register(&key("func_70070_b"), |c| entity_light(c, true, true));
    }
    register("net/minecraft/entity/EntityLivingBase.func_70668_bt", creature_attribute);
    register("*.func_70668_bt", creature_attribute);
    register("net/minecraft/entity/EntityLivingBase.func_70617_f_", on_ladder);
    register("*.func_70617_f_", on_ladder);

    for class in ["entity/player/EntityPlayer", "client/entity/AbstractClientPlayer", "client/entity/EntityPlayerSP"] {
        register(&format!("net/minecraft/{class}.func_175140_cp"), |c| {
            Ok(Ret::Bool(z(c.env, &c.this, "field_175153_bG")?))
        });
        register(&format!("net/minecraft/{class}.func_175142_cm"), |c| {
            let caps = c.env.get_field(&c.this, "field_71075_bZ", CAPABILITIES)?.l()?;
            Ok(Ret::Bool(!caps.is_null() && z(c.env, &caps, "field_75099_e")?))
        });
    }
    register("net/minecraft/client/entity/AbstractClientPlayer.func_70618_n", remove_potion_client);

    // Critical-hit feedback is a client particle burst; our effect renderer has no mirror to ask for one.
    for method in ["func_71009_b", "func_71047_c"] {
        register(&format!("net/minecraft/client/entity/EntityPlayerSP.{method}"), |_| Ok(Ret::Null));
    }
    // dropOneItem sends a C07 action and returns the (server-made) EntityItem, which is null on the client; the
    // client's own drop path is its input code, so there is nothing to queue here.
    register("net/minecraft/client/entity/EntityPlayerSP.func_71040_bB", |_| Ok(Ret::Null));
    // closeScreen sends C0D and clears the cursor stack, then displayGuiScreen(null): the screen close is the part
    // this client can perform, and its container code does the rest.
    register("net/minecraft/client/entity/EntityPlayerSP.func_71053_j", |c| {
        game::open_screen(c.env, &JObject::null())?;
        Ok(Ret::Null)
    });
}

/// Constructors are void: `Entities.invoke` fills the mod-built object, its null result is discarded.
fn invoke(c: &mut Call<'_, '_>, key: &str) -> Result<()> {
    let name = c.env.new_string(key)?;
    let this = c.env.new_local_ref(&c.this)?;
    let args = JObject::from(c.env.new_local_ref(c.arguments())?);
    rt::call_static(c.env, "rustmc/forge/Entities", "invoke", "(Ljava/lang/String;Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;",
        &[JValue::Object(&name), JValue::Object(&this), JValue::Object(&args)])?;
    Ok(())
}

/// `Entity.getHorizontalFacing`: `EnumFacing.getHorizontal(floor_double(yaw * 4 / 360 + 0.5) & 3)`.
fn horizontal_facing(c: &mut Call<'_, '_>) -> Result<Ret> {
    let yaw = f(c.env, &c.this, "field_70177_z")?;
    let index = ((yaw * 4.0 / 360.0) as f64 + 0.5).floor() as i32 & 3;
    let facing = rt::call_static(c.env, "net/minecraft/util/EnumFacing", "func_176731_b",
        "(I)Lnet/minecraft/util/EnumFacing;", &[JValue::Int(index)])?.l()?;
    Ok(Ret::Obj(c.env.new_global_ref(facing)?))
}

/// `Entity.getLook`: interpolate pitch/yaw, then `getVectorForRotation` through MathHelper's sin table.
fn get_look(c: &mut Call<'_, '_>) -> Result<Ret> {
    let ticks = c.f32(0)?;
    let (mut pitch, mut yaw) = (f(c.env, &c.this, "field_70125_A")?, f(c.env, &c.this, "field_70177_z")?);
    if ticks != 1.0 {
        let (prev_pitch, prev_yaw) = (f(c.env, &c.this, "field_70127_C")?, f(c.env, &c.this, "field_70126_B")?);
        pitch = prev_pitch + (pitch - prev_pitch) * ticks;
        yaw = prev_yaw + (yaw - prev_yaw) * ticks;
    }
    let yaw_angle = -yaw * 0.017453292 - std::f32::consts::PI;
    let cos_yaw = trig(c.env, "func_76134_b", yaw_angle)?;
    let sin_yaw = trig(c.env, "func_76126_a", yaw_angle)?;
    let cos_pitch = -trig(c.env, "func_76134_b", -pitch * 0.017453292)?;
    let sin_pitch = trig(c.env, "func_76126_a", -pitch * 0.017453292)?;
    let vec = rt::new_object(c.env, "net/minecraft/util/Vec3", "(DDD)V", &[
        JValue::Double((sin_yaw * cos_pitch) as f64), JValue::Double(sin_pitch as f64),
        JValue::Double((cos_yaw * cos_pitch) as f64)])?;
    Ok(Ret::Obj(c.env.new_global_ref(vec)?))
}

fn trig(env: &mut JNIEnv<'_>, method: &str, angle: f32) -> Result<f32> {
    Ok(rt::call_static(env, "net/minecraft/util/MathHelper", method, "(F)F", &[JValue::Float(angle)])?.f()?)
}

/// `EntityArmorStand.hasMarker`: data watcher byte 10, bit 4.
fn has_marker(c: &mut Call<'_, '_>) -> Result<Ret> {
    let watcher = c.env.get_field(&c.this, "field_70180_af", DATA_WATCHER)?.l()?;
    if watcher.is_null() || !watches(c.env, &watcher, 10)? { return Ok(Ret::Bool(false)); }
    Ok(Ret::Bool(c.env.call_method(&watcher, "func_75683_a", "(I)B", &[JValue::Int(10)])?.b()? & 16 != 0))
}

/// `EntityItemFrame.getDisplayedItem`: data watcher slot 8; our frames mirror no item, so it is null.
fn displayed_item(c: &mut Call<'_, '_>) -> Result<Ret> {
    let watcher = c.env.get_field(&c.this, "field_70180_af", DATA_WATCHER)?.l()?;
    if watcher.is_null() || !watches(c.env, &watcher, 8)? { return Ok(Ret::Null); }
    let item = c.env.call_method(&watcher, "func_82710_f", &format!("(I){ITEM_STACK}"), &[JValue::Int(8)])?.l()?;
    Ok(if item.is_null() { Ret::Null } else { Ret::Obj(c.env.new_global_ref(item)?) })
}

/// `DataWatcher`'s getters dereference the entry they are asked for; ours holds only the ids Live mirrors.
fn watches(env: &mut JNIEnv<'_>, watcher: &JObject<'_>, id: i32) -> Result<bool> {
    let entries = env.get_field(watcher, "field_75695_b", MAP)?.l()?;
    let key = rt::new_object(env, "java/lang/Integer", "(I)V", &[JValue::Int(id)])?;
    Ok(env.call_method(&entries, "containsKey", "(Ljava/lang/Object;)Z", &[JValue::Object(&key)])?.z()?)
}

/// `EntityLivingBase.isOnLadder`: the block at the feet is a ladder or a vine. The player term of vanilla's test
/// needs a spectator flag the client never mirrors, and no player here is one.
fn on_ladder(c: &mut Call<'_, '_>) -> Result<Ret> {
    let world = c.env.get_field(&c.this, "field_70170_p", WORLD)?.l()?;
    let bounds = c.env.get_field(&c.this, "field_70121_D", AABB)?.l()?;
    if world.is_null() || bounds.is_null() { return Ok(Ret::Bool(false)); }
    let (x, y, z) = (d(c.env, &c.this, "field_70165_t")?, d(c.env, &bounds, "field_72338_b")?,
        d(c.env, &c.this, "field_70161_v")?);
    let pos = rt::new_object(c.env, "net/minecraft/util/BlockPos", "(III)V", &[
        JValue::Int(x.floor() as i32), JValue::Int(y.floor() as i32), JValue::Int(z.floor() as i32)])?;
    let state = c.env.call_method(&world, "func_180495_p", GET_BLOCK_STATE, &[JValue::Object(&pos)])?.l()?;
    let block = c.env.call_method(&state, "func_177230_c", &format!("(){BLOCK}"), &[])?.l()?;
    let ladder = rt::static_field(c.env, "net/minecraft/init/Blocks", "field_150468_ap", BLOCK)?.l()?;
    let vine = rt::static_field(c.env, "net/minecraft/init/Blocks", "field_150395_bd", BLOCK)?.l()?;
    Ok(Ret::Bool(c.env.is_same_object(&block, &ladder)? || c.env.is_same_object(&block, &vine)?))
}

/// `EntityLivingBase.getCreatureAttribute`: UNDEFINED, but 1.8.9's undead and arthropod mobs override it.
fn creature_attribute(c: &mut Call<'_, '_>) -> Result<Ret> {
    let mut kind = "UNDEFINED";
    for (class, attribute) in [
        ("net/minecraft/entity/monster/EntityZombie", "UNDEAD"),
        ("net/minecraft/entity/monster/EntitySkeleton", "UNDEAD"),
        ("net/minecraft/entity/boss/EntityWither", "UNDEAD"),
        ("net/minecraft/entity/monster/EntitySpider", "ARTHROPOD"),
        ("net/minecraft/entity/monster/EntitySilverfish", "ARTHROPOD"),
        ("net/minecraft/entity/monster/EntityEndermite", "ARTHROPOD"),
    ] {
        if c.env.is_instance_of(&c.this, class)? { kind = attribute; break; }
    }
    let attribute = rt::static_field(c.env, "net/minecraft/entity/EnumCreatureAttribute", kind,
        "Lnet/minecraft/entity/EnumCreatureAttribute;")?.l()?;
    Ok(Ret::Obj(c.env.new_global_ref(attribute)?))
}

/// `EntityLivingBase.removePotionEffectClient`: drop the effect from the map mods read back.
fn remove_potion_client(c: &mut Call<'_, '_>) -> Result<Ret> {
    let id = c.i32(0)?;
    let effects = c.env.get_field(&c.this, "field_70713_bf", MAP)?.l()?;
    if !effects.is_null() {
        let key = rt::new_object(c.env, "java/lang/Integer", "(I)V", &[JValue::Int(id)])?;
        c.env.call_method(&effects, "remove", "(Ljava/lang/Object;)Ljava/lang/Object;", &[JValue::Object(&key)])?;
    }
    Ok(Ret::Null)
}

fn d(env: &mut JNIEnv<'_>, object: &JObject<'_>, field: &str) -> Result<f64> { env.get_field(object, field, "D")?.d() }
fn f(env: &mut JNIEnv<'_>, object: &JObject<'_>, field: &str) -> Result<f32> { env.get_field(object, field, "F")?.f() }
fn z(env: &mut JNIEnv<'_>, object: &JObject<'_>, field: &str) -> Result<bool> { env.get_field(object, field, "Z")?.z() }
