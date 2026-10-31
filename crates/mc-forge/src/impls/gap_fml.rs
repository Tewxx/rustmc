//! FML's loader, logging, deobfuscation remapper and ASM discovery, plus the stray vanilla leaves mods read.
//! `rustmc.forge.Fml` owns the mod list and the log4j sink; everything else is vanilla over mirrored stub fields.
use crate::{runtime as rt, stubs::{register, Call, Ret}};
use jni::{errors::Result, objects::{GlobalRef, JValue}};
use std::sync::OnceLock;

const FML: &str = "rustmc/forge/Fml";

fn obj_of(c: &mut Call<'_, '_>, field: &str, sig: &str) -> Result<Option<GlobalRef>> {
    let o = c.env.get_field(&c.this, field, sig)?.l()?;
    Ok(if o.is_null() { None } else { Some(c.env.new_global_ref(&o)?) })
}

/// A field read for a handler that just returns it (null when `Live` never mirrored it).
fn field_ret(c: &mut Call<'_, '_>, field: &str, sig: &str) -> Result<Ret> {
    Ok(obj_of(c, field, sig)?.map_or(Ret::Null, Ret::Obj))
}

/// Call `rustmc.forge.Fml.<name>` and hand back the result.
fn fml<'l>(c: &mut Call<'_, 'l>, name: &str, sig: &str, args: &[JValue]) -> Result<jni::objects::JValueOwned<'l>> {
    let cls = rt::class(c.env, FML)?;
    c.env.call_static_method(&cls, name, sig, args)
}

fn same_string(c: &mut Call<'_, '_>, i: usize) -> Result<Ret> {
    let o = c.obj(i)?;
    Ok(if o.is_null() { Ret::Null } else { Ret::Obj(c.env.new_global_ref(&o)?) })
}

/// World methods WorldClient inherits: the stub in either class dispatches to the same handler.
macro_rules! world {
    ($m:literal, $f:expr) => {
        register(concat!("net/minecraft/world/World.", $m), $f);
        register(concat!("net/minecraft/client/multiplayer/WorldClient.", $m), $f);
    };
}

fn chunk(c: &mut Call<'_, '_>, x: i32, z: i32) -> Result<Ret> {
    let this = c.env.new_local_ref(&c.this)?;
    let chunk = rt::call_static(c.env, "rustmc/forge/Live", "chunk", "(Ljava/lang/Object;II)Ljava/lang/Object;",
        &[JValue::Object(&this), JValue::Int(x), JValue::Int(z)])?.l()?;
    Ok(Ret::Obj(c.env.new_global_ref(chunk)?))
}

fn ray_trace(c: &mut Call<'_, '_>, liquid: bool, ignore_no_box: bool, return_last: bool) -> Result<Ret> {
    let (this, a, b) = (c.env.new_local_ref(&c.this)?, c.obj(0)?, c.obj(1)?);
    let hit = rt::call_static(c.env, "rustmc/forge/Live", "rayTrace",
        "(Lnet/minecraft/world/World;Lnet/minecraft/util/Vec3;Lnet/minecraft/util/Vec3;ZZZ)Ljava/lang/Object;",
        &[JValue::Object(&this), JValue::Object(&a), JValue::Object(&b), JValue::Bool(liquid as u8), JValue::Bool(ignore_no_box as u8), JValue::Bool(return_last as u8)])?.l()?;
    Ok(if hit.is_null() { Ret::Null } else { Ret::Obj(c.env.new_global_ref(hit)?) })
}

pub(crate) fn register_all() {
    loader();
    worldclient();
    remapper();
    logging();
    discovery();
    leaves();
}

/// `Loader`: no mod list lives in the stub instance, so instance/ids/containers come from `Fml`.
fn loader() {
    register("net/minecraftforge/fml/common/Loader.instance", |c| {
        static INSTANCE: OnceLock<GlobalRef> = OnceLock::new();
        if let Some(g) = INSTANCE.get() { return Ok(Ret::Obj(g.clone())); }
        let cls = rt::class(c.env, "net/minecraftforge/fml/common/Loader")?;
        let o = c.env.alloc_object(&cls)?;
        let g = c.env.new_global_ref(&o)?;
        let _ = INSTANCE.set(g.clone());
        Ok(Ret::Obj(g))
    });
    register("net/minecraftforge/fml/common/Loader.isModLoaded", |c| {
        let id = c.obj(0)?;
        Ok(Ret::Bool(fml(c, "isLoaded", "(Ljava/lang/String;)Z", &[JValue::Object(&id)])?.z()?))
    });
    register("net/minecraftforge/fml/common/Loader.getActiveModList", |c| {
        let l = fml(c, "mods", "()Ljava/util/List;", &[])?.l()?;
        Ok(Ret::Obj(c.env.new_global_ref(&l)?))
    });
}

/// WorldClient: what the live graph mirrors, plus the vanilla answer when it does not (see report).
fn worldclient() {
    // loadedEntityList: Live.sync fills it every tick.
    world!("func_72910_y", |c| field_ret(c, "field_72996_f", "Ljava/util/List;"));
    world!("func_72827_u", |c| {
        let n = match obj_of(c, "field_72996_f", "Ljava/util/List;")? {
            Some(l) => c.env.call_method(l.as_obj(), "size", "()I", &[])?.i()?,
            None => 0,
        };
        Ok(Ret::Str(format!("All: {}", n)))
    });
    // No chunk provider, provider, chunks or tile entities are mirrored: vanilla would never see null here.
    world!("func_72863_F", |c| field_ret(c, "field_73020_y", "Lnet/minecraft/world/chunk/IChunkProvider;"));
    world!("func_72912_H", |c| field_ret(c, "field_72986_A", "Lnet/minecraft/world/storage/WorldInfo;"));
    world!("func_72959_q", |c| field_ret(c, "field_73011_w", "Lnet/minecraft/world/biome/WorldChunkManager;"));
    // getChunkFromBlockCoords / getChunkFromChunkCoords: Live's per-column Chunk over the mirror.
    world!("func_175726_f", |c| {
        let pos = c.obj(0)?;
        let x = c.env.call_method(&pos, "func_177958_n", "()I", &[])?.i()? >> 4;
        let z = c.env.call_method(&pos, "func_177952_p", "()I", &[])?.i()? >> 4;
        chunk(c, x, z)
    });
    world!("func_72964_e", |c| { let (x, z) = (c.i32(0)?, c.i32(1)?); chunk(c, x, z) });
    world!("func_175625_s", |_| Ok(Ret::Null));
    world!("func_72981_t", |_| Ok(Ret::Null));
    // The connection lives in Rust; the NetHandlerPlayClient in the live graph has no NetworkManager.
    world!("func_72882_A", |_| Ok(Ret::Null));
    // rayTraceBlocks (2-, 3- and 5-argument forms): Live.rayTrace walks the mirrored blocks.
    world!("func_72933_a", |c| ray_trace(c, false, false, false));
    world!("func_72901_a", |c| { let liquid = c.bool(2)?; ray_trace(c, liquid, false, false) });
    world!("func_147447_a", |c| { let (l, n, r) = (c.bool(2)?, c.bool(3)?, c.bool(4)?); ray_trace(c, l, n, r) });

    register("net/minecraft/client/multiplayer/WorldClient.<init>", |c| {
        // vanilla: super() marks the world remote and builds WorldInfo(settings, "MpServer")
        c.env.set_field(&c.this, "field_72995_K", "Z", JValue::Bool(1))?;
        let settings = c.obj(1)?;
        if !settings.is_null() {
            let name = c.env.new_string("MpServer")?;
            let info = rt::new_object(c.env, "net/minecraft/world/storage/WorldInfo",
                "(Lnet/minecraft/world/WorldSettings;Ljava/lang/String;)V", &[JValue::Object(&settings), JValue::Object(&name)])?;
            c.env.set_field(&c.this, "field_72986_A", "Lnet/minecraft/world/storage/WorldInfo;", JValue::Object(&info))?;
        }
        Ok(Ret::Null)
    });
}

/// Names are already SRG at runtime, so FML's remapper is the identity here (the map it consults is empty).
fn remapper() {
    register("net/minecraftforge/fml/common/asm/transformers/deobf/FMLDeobfuscatingRemapper.map", |c| same_string(c, 0));
    register("net/minecraftforge/fml/common/asm/transformers/deobf/FMLDeobfuscatingRemapper.mapMethodDesc", |c| same_string(c, 0));
    register("net/minecraftforge/fml/common/asm/transformers/deobf/FMLDeobfuscatingRemapper.mapFieldName", |c| same_string(c, 1));
    register("net/minecraftforge/fml/common/asm/transformers/deobf/FMLDeobfuscatingRemapper.mapMethodName", |c| same_string(c, 1));
}

fn logging() {
    register("net/minecraftforge/fml/common/FMLLog.fine", |c| {
        let format = c.obj(0)?;
        let data = c.obj(1)?;
        fml(c, "fine", "(Ljava/lang/String;[Ljava/lang/Object;)V", &[JValue::Object(&format), JValue::Object(&data)])?;
        Ok(Ret::Null)
    });
    register("net/minecraftforge/fml/common/FMLLog.log", |c| {
        let (level, throwable, format, data) = (c.obj(0)?, c.obj(1)?, c.obj(2)?, c.obj(3)?);
        fml(c, "log", "(Lorg/apache/logging/log4j/Level;Ljava/lang/Throwable;Ljava/lang/String;[Ljava/lang/Object;)V",
            &[JValue::Object(&level), JValue::Object(&throwable), JValue::Object(&format), JValue::Object(&data)])?;
        Ok(Ret::Null)
    });
}

/// ASM discovery objects are only filled when FML built them: read the fields the parser writes.
fn discovery() {
    register("net/minecraftforge/fml/common/discovery/asm/ASMModParser.getASMType", |c| {
        Ok(match c.env.get_field(&c.this, "asmType", "Lorg/objectweb/asm/Type;").and_then(|v| v.l()) {
            Ok(o) if !o.is_null() => Ret::Obj(c.env.new_global_ref(&o)?),
            _ => Ret::Null,
        })
    });
    register("net/minecraftforge/fml/common/discovery/asm/ASMModParser.getAnnotations", |c| {
        Ok(match c.env.get_field(&c.this, "annotations", "Ljava/util/LinkedList;").and_then(|v| v.l()) {
            Ok(o) if !o.is_null() => Ret::Obj(c.env.new_global_ref(&o)?),
            _ => Ret::Null,
        })
    });
    register("net/minecraftforge/fml/common/discovery/asm/ModAnnotation.getASMType", |c| {
        Ok(match c.env.get_field(&c.this, "asmType", "Lorg/objectweb/asm/Type;").and_then(|v| v.l()) {
            Ok(o) if !o.is_null() => Ret::Obj(c.env.new_global_ref(&o)?),
            _ => Ret::Null,
        })
    });
    register("net/minecraftforge/fml/common/discovery/asm/ModAnnotation.getValues", |c| {
        Ok(match c.env.get_field(&c.this, "values", "Ljava/util/Map;").and_then(|v| v.l()) {
            Ok(o) if !o.is_null() => Ret::Obj(c.env.new_global_ref(&o)?),
            _ => Ret::Null,
        })
    });
}

fn leaves() {
    register("net/minecraftforge/fml/common/FMLCommonHandler.getBrandings", |c| {
        let include_mc = c.bool(0)?;
        let list = fml(c, "brandings", "(Z)Ljava/util/List;", &[JValue::Bool(include_mc.into())])?.l()?;
        Ok(Ret::Obj(c.env.new_global_ref(&list)?))
    });
    // Forge's own version constants are real class data (ConstantValue), not something to invent.
    register("net/minecraftforge/common/ForgeVersion.getVersion", |c| {
        let cls = rt::class(c.env, "net/minecraftforge/common/ForgeVersion")?;
        let (major, minor, rev, build) = (
            c.env.get_static_field(&cls, "majorVersion", "I")?.i()?,
            c.env.get_static_field(&cls, "minorVersion", "I")?.i()?,
            c.env.get_static_field(&cls, "revisionVersion", "I")?.i()?,
            c.env.get_static_field(&cls, "buildVersion", "I")?.i()?,
        );
        Ok(Ret::Str(format!("{}.{}.{}.{}", major, minor, rev, build)))
    });
    register("net/minecraftforge/common/capabilities/CapabilityDispatcher.hasCapability", |c| {
        let (cap, facing) = (c.obj(0)?, c.obj(1)?);
        let found = (|| -> Result<bool> {
            let caps = jni::objects::JObjectArray::from(c.env.get_field(&c.this, "caps", "[Lnet/minecraftforge/common/capabilities/ICapabilityProvider;")?.l()?);
            if caps.is_null() { return Ok(false); }
            for i in 0..c.env.get_array_length(&caps)? {
                let p = c.env.get_object_array_element(&caps, i)?;
                if p.is_null() { continue; }
                let has = c.env.call_method(&p, "hasCapability",
                    "(Lnet/minecraftforge/common/capabilities/Capability;Lnet/minecraft/util/EnumFacing;)Z",
                    &[JValue::Object(&cap), JValue::Object(&facing)])?.z()?;
                if has { return Ok(true); }
            }
            Ok(false)
        })();
        Ok(Ret::Bool(found.unwrap_or(false)))
    });

    // SoundCategory's name differs from the enum constant for RECORDS/BLOCKS; its ctor is a stub, so map ordinals.
    register("net/minecraft/client/audio/SoundCategory.func_147155_a", |c| {
        const NAMES: [&str; 10] = ["master", "music", "record", "weather", "block", "hostile", "neutral", "player", "ambient", "voice"];
        let i = c.env.call_method(&c.this, "ordinal", "()I", &[])?.i()?;
        Ok(NAMES.get(i as usize).map_or(Ret::Null, |n| Ret::Str((*n).into())))
    });
    register("net/minecraft/client/gui/FontRenderer.func_82883_a", |c| {
        Ok(Ret::Bool(c.env.get_field(&c.this, "field_78293_l", "Z")?.z()?))
    });
    register("net/minecraft/client/gui/GuiScreenWorking.<init>", |c| {
        let empty = c.env.new_string("")?;
        c.env.set_field(&c.this, "field_146591_a", "Ljava/lang/String;", JValue::Object(&empty))?;
        c.env.set_field(&c.this, "field_146589_f", "Ljava/lang/String;", JValue::Object(&empty))?;
        Ok(Ret::Null)
    });
    register("net/minecraft/client/gui/MapItemRenderer.func_148249_a", |c| {
        if let Some(maps) = obj_of(c, "field_148252_c", "Ljava/util/Map;")? {
            c.env.call_method(maps.as_obj(), "clear", "()V", &[])?;
        }
        Ok(Ret::Null)
    });
    register("net/minecraft/client/gui/ServerSelectionList.func_148180_b", |c| {
        let mut i = c.i32(0)?;
        let Some(internet) = obj_of(c, "field_148198_l", "Ljava/util/List;")? else { return Ok(Ret::Null) };
        let n = c.env.call_method(internet.as_obj(), "size", "()I", &[])?.i()?;
        if i < 0 { return Ok(Ret::Null); }
        if i < n {
            let e = c.env.call_method(internet.as_obj(), "get", "(I)Ljava/lang/Object;", &[JValue::Int(i)])?.l()?;
            return Ok(if e.is_null() { Ret::Null } else { Ret::Obj(c.env.new_global_ref(&e)?) });
        }
        i -= n;
        if i == 0 {
            return field_ret(c, "field_148196_n", "Lnet/minecraft/client/gui/GuiListExtended$IGuiListEntry;");
        }
        let Some(lan) = obj_of(c, "field_148199_m", "Ljava/util/List;")? else { return Ok(Ret::Null) };
        let n = c.env.call_method(lan.as_obj(), "size", "()I", &[])?.i()?;
        if i - 1 >= n { return Ok(Ret::Null); }
        let e = c.env.call_method(lan.as_obj(), "get", "(I)Ljava/lang/Object;", &[JValue::Int(i - 1)])?.l()?;
        Ok(if e.is_null() { Ret::Null } else { Ret::Obj(c.env.new_global_ref(&e)?) })
    });
    register("net/minecraft/client/gui/inventory/GuiContainer.getSlotUnderMouse", |c| {
        field_ret(c, "field_147006_u", "Lnet/minecraft/inventory/Slot;")
    });
    // The camera (ActiveRenderInfo.position) is not mirrored, and IBlockState.getBlock has no handler yet.
    register("net/minecraft/client/renderer/ActiveRenderInfo.func_180786_a", |_| Ok(Ret::Null));
    register("net/minecraft/client/renderer/vertex/VertexFormat.func_181719_f", |c| {
        Ok(Ret::Int(c.env.get_field(&c.this, "field_177353_d", "I")?.i()? / 4))
    });
    register("net/minecraft/network/ServerStatusResponse.func_151322_c", |c| {
        field_ret(c, "field_151325_c", "Lnet/minecraft/network/ServerStatusResponse$MinecraftProtocolVersionIdentifier;")
    });
    register("net/minecraft/network/ServerStatusResponse$MinecraftProtocolVersionIdentifier.func_151304_b", |c| {
        Ok(Ret::Int(c.env.get_field(&c.this, "field_151305_b", "I")?.i()?))
    });
}
