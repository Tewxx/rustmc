//! Forge API surface mods reach for world, chunks, tile entities: methods the corpus scan found unimplemented.
use crate::{runtime as rt, stubs::{register, Ret}};
use jni::{objects::{JObject, JValue}, JNIEnv};

const POS: &str = "Lnet/minecraft/util/BlockPos;";

/// Stub constructors never assign fields, so a read must not throw: absent state reads as "unset".
fn fobj<'l>(env: &mut JNIEnv<'l>, o: &JObject<'l>, name: &str, sig: &str) -> Option<JObject<'l>> {
    match env.get_field(o, name, sig) { Ok(v) => v.l().ok(), Err(_) => { let _ = env.exception_clear(); None } }
}
fn fbool(env: &mut JNIEnv<'_>, o: &JObject<'_>, name: &str) -> Option<bool> {
    match env.get_field(o, name, "Z") { Ok(v) => v.z().ok(), Err(_) => { let _ = env.exception_clear(); None } }
}

fn ret_obj(env: &mut JNIEnv<'_>, o: JObject<'_>) -> Ret {
    if o.is_null() { return Ret::Null; }
    match env.new_global_ref(&o) { Ok(g) => Ret::Obj(g), Err(_) => { let _ = env.exception_clear(); Ret::Null } }
}

/// `Live` owns the mirrored block states; ask it for one, else null (nothing else can name the state).
fn live_state<'l>(env: &mut JNIEnv<'l>, pos: &JObject<'l>) -> Ret {
    live(env, "blockState", pos)
}

fn live<'l>(env: &mut JNIEnv<'l>, method: &str, pos: &JObject<'l>) -> Ret {
    match rt::call_static(env, "rustmc/forge/Live", method, "(Lnet/minecraft/util/BlockPos;)Ljava/lang/Object;", &[JValue::Object(pos)])
        .and_then(|v| v.l()) {
        Ok(s) => ret_obj(env, s),
        Err(_) => { let _ = env.exception_clear(); Ret::Null }
    }
}

/// `EnumBannerPattern.getPatternID`: the vanilla id table, keyed by enum constant name.
fn pattern_id(name: &str) -> Option<&'static str> {
    Some(match name {
        "BASE" => "b", "SQUARE_BOTTOM_LEFT" => "bl", "SQUARE_BOTTOM_RIGHT" => "br",
        "SQUARE_TOP_LEFT" => "tl", "SQUARE_TOP_RIGHT" => "tr", "STRIPE_BOTTOM" => "bs",
        "STRIPE_TOP" => "ts", "STRIPE_LEFT" => "ls", "STRIPE_RIGHT" => "rs",
        "STRIPE_CENTER" => "cs", "STRIPE_MIDDLE" => "ms", "STRIPE_DOWNRIGHT" => "drs",
        "STRIPE_DOWNLEFT" => "dls", "STRIPE_SMALL" => "ss", "CROSS" => "cr",
        "STRAIGHT_CROSS" => "sc", "TRIANGLE_BOTTOM" => "bt", "TRIANGLE_TOP" => "tt",
        "TRIANGLES_BOTTOM" => "bts", "TRIANGLES_TOP" => "tts", "DIAGONAL_LEFT" => "ld",
        "DIAGONAL_RIGHT" => "rd", "DIAGONAL_LEFT_MIRROR" => "lud", "DIAGONAL_RIGHT_MIRROR" => "rud",
        "CIRCLE_MIDDLE" => "mc", "RHOMBUS_MIDDLE" => "mr", "HALF_VERTICAL" => "vh",
        "HALF_HORIZONTAL" => "hh", "HALF_VERTICAL_MIRROR" => "vhr",
        "HALF_HORIZONTAL_MIRROR" => "hhb", "BORDER" => "bo", "CURLY_BORDER" => "cbo",
        "CREEPER" => "cre", "GRADIENT" => "gra", "GRADIENT_UP" => "gru", "BRICKS" => "bri",
        "SKULL" => "sku", "FLOWER" => "flo", "MOJANG" => "moj",
        _ => return None,
    })
}

pub(crate) fn register_all() {
    // TileEntity
    register("net/minecraft/tileentity/TileEntity.func_174877_v", |c| {
        let pos = fobj(c.env, &c.this, "field_174879_c", POS);
        Ok(match pos { Some(p) => ret_obj(c.env, p), None => Ret::Null })
    });
    register("net/minecraft/tileentity/TileEntity.func_145838_q", |c| {
        // Vanilla caches `worldObj.getBlockState(pos).getBlock()`; only a real assignment can be answered.
        let block = fobj(c.env, &c.this, "field_145854_h", "Lnet/minecraft/block/Block;");
        Ok(match block { Some(b) => ret_obj(c.env, b), None => Ret::Null })
    });

    // Chunk
    register("net/minecraft/world/chunk/Chunk.func_177435_g", |c| {
        let pos = c.obj(0)?;
        Ok(live_state(c.env, &pos))
    });
    register("net/minecraft/world/chunk/Chunk.func_177411_a", |c| {
        // The mirrored biome column (plains when unassigned), as World.getBiomeGenForCoords answers.
        let pos = c.obj(0)?;
        Ok(live(c.env, "biome", &pos))
    });
    // Chunk.getLightFor / getLightSubtracted over Live's light mirror (the chunk is the one holding `pos`).
    register("net/minecraft/world/chunk/Chunk.func_177413_a", |c| {
        let (kind, pos) = (c.obj(0)?, c.obj(1)?);
        let sky = rt::static_field(c.env, "net/minecraft/world/EnumSkyBlock", "SKY", "Lnet/minecraft/world/EnumSkyBlock;")?.l()?;
        let sky = c.env.is_same_object(&kind, &sky)?;
        Ok(Ret::Int(rt::call_static(c.env, "rustmc/forge/Live", "lightFor", "(ZLnet/minecraft/util/BlockPos;)I", &[JValue::Bool(sky as u8), JValue::Object(&pos)])?.i()?))
    });
    register("net/minecraft/world/chunk/Chunk.func_177443_a", |c| {
        let (pos, amount) = (c.obj(0)?, c.i32(1)?);
        Ok(Ret::Int(rt::call_static(c.env, "rustmc/forge/Live", "lightSubtracted", "(Lnet/minecraft/util/BlockPos;I)I", &[JValue::Object(&pos), JValue::Int(amount)])?.i()?))
    });
    register("net/minecraft/world/chunk/Chunk.func_76587_i", |c| {
        let arrays = fobj(c.env, &c.this, "field_76652_q", "[Lnet/minecraft/world/chunk/storage/ExtendedBlockStorage;");
        Ok(match arrays { Some(a) => ret_obj(c.env, a), None => Ret::Null })
    });

    // ExtendedBlockStorage.get: needs the id->state table and the section chars, neither mirrored.
    register("net/minecraft/world/chunk/storage/ExtendedBlockStorage.func_177485_a", |_c| Ok(Ret::Null));

    // World
    register("net/minecraft/world/World.func_72877_b", |c| {
        let time = c.i64(0)?;
        if let Some(info) = fobj(c.env, &c.this, "field_72986_A", "Lnet/minecraft/world/storage/WorldInfo;") {
            if !info.is_null() { c.env.call_method(&info, "func_76068_b", "(J)V", &[JValue::Long(time)])?; }
        }
        Ok(Ret::Null)
    });
    register("net/minecraft/world/World.func_72894_k", |_c| Ok(Ret::Null)); // rainingStrength: no mirror field
    register("net/minecraft/world/World.func_72942_c", |_c| Ok(Ret::Bool(false))); // weatherEffects: no mirror field
    register("net/minecraft/world/World.func_72980_b", |_c| Ok(Ret::Null)); // vanilla body is empty

    // WorldProvider.doesXZShowFog and WorldInfo.getLastTimePlayed defaults
    register("net/minecraft/world/WorldProvider.func_76568_b", |_c| Ok(Ret::Bool(false)));
    register("net/minecraft/world/storage/WorldInfo.func_82573_f", |_c| Ok(Ret::Long(0)));

    // TileEntityBanner: banner data lives in Rust-owned state, so only an already built field answers.
    register("net/minecraft/tileentity/TileEntityBanner.func_175110_d", |_c| Ok(Ret::Null)); // colorList is Rust-owned
    register("net/minecraft/tileentity/TileEntityBanner.func_175114_c", |_c| Ok(Ret::Null)); // patternList is Rust-owned
    register("net/minecraft/tileentity/TileEntityBanner.func_175116_e", |c| {
        if let Some(s) = fobj(c.env, &c.this, "field_175121_j", "Ljava/lang/String;") {
            if !s.is_null() {
                let text: String = c.env.get_string(&s.into())?.into();
                return Ok(Ret::Str(text));
            }
        }
        // Vanilla sets "" while the banner data is uninitialised.
        Ok(Ret::Str(String::new()))
    });
    register("net/minecraft/tileentity/TileEntityBanner$EnumBannerPattern.func_177271_a", |c| {
        let name = c.env.call_method(&c.this, "name", "()Ljava/lang/String;", &[])?.l()?;
        let name: String = c.env.get_string(&name.into())?.into();
        Ok(match pattern_id(&name) { Some(id) => Ret::Str(id.into()), None => Ret::Null })
    });

    register("net/minecraft/client/renderer/tileentity/TileEntityBannerRenderer$TimedBannerTexture.<init>", |_c| Ok(Ret::Null));
}
