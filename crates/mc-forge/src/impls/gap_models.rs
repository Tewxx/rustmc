//! Forge API surface mods reach for resource packs, texture atlas, baked models, model classes, item/entity
//! renderers, particles: methods the corpus scan found unimplemented.
//!
//! Vanilla keeps this state in Java fields, so most bodies below read/write the same SRG fields vanilla does. Where
//! the object graph a vanilla body needs was never built (no bake pass, no particle pass, no entity render pass)
//! the method does the part that is observable and leaves the rest, returning the vanilla answer for that state.
use crate::{runtime as rt, stubs::{register, Ret}};
use jni::{errors::Result, objects::{JObject, JValue}, JNIEnv};

const RPR: &str = "net/minecraft/client/resources/ResourcePackRepository$Entry";
const RM_SIG: &str = "Lnet/minecraft/client/model/ModelRenderer;";
const MODEL_SIG: &str = "Lnet/minecraft/client/resources/model/IBakedModel;";
const SPRITE_SIG: &str = "Lnet/minecraft/client/renderer/texture/TextureAtlasSprite;";
const BUILDER: &str = "net/minecraft/client/resources/model/SimpleBakedModel$Builder";
const BAKED: &str = "net/minecraft/client/resources/model/SimpleBakedModel";

/// `String.hashCode` over UTF-16 units, so a name we join ourselves hashes like vanilla's `String.format` result.
fn java_hash(s: &str) -> i32 {
    s.encode_utf16().fold(0i32, |h, u| h.wrapping_mul(31).wrapping_add(u as i32))
}

fn field<'l>(env: &mut JNIEnv<'l>, obj: &JObject<'l>, name: &str, sig: &str) -> Result<JObject<'l>> {
    env.get_field(obj, name, sig)?.l()
}

/// A call into a stub whose SRG name is not exercised elsewhere: a missing target answers like an absent value
/// instead of leaving a pending exception in the VM.
fn call_opt<'l>(env: &mut JNIEnv<'l>, obj: &JObject<'l>, name: &str, sig: &str) -> Result<JObject<'l>> {
    match env.call_method(obj, name, sig, &[]) {
        Ok(v) => v.l(),
        Err(_) => { let _ = env.exception_clear(); Ok(JObject::null()) }
    }
}

/// A boolean query on a stub whose SRG name is not exercised elsewhere: an absent answer reads as false.
fn call_bool(env: &mut JNIEnv<'_>, obj: &JObject<'_>, name: &str) -> bool {
    match env.call_method(obj, name, "()Z", &[]) {
        Ok(v) => v.z().unwrap_or(false),
        Err(_) => { let _ = env.exception_clear(); false }
    }
}

#[allow(dead_code)]
/// One `ModelRenderer` of a `ModelBiped` graph: vanilla's box sizes never reach a rasteriser here, so only the
/// rotation point and mirror flag mods read/write are set.
fn model_renderer<'l>(env: &mut JNIEnv<'l>, base: &JObject<'l>, p: [f32; 3], mirror: bool) -> Result<JObject<'l>> {
    let r = rt::new_object(env, "net/minecraft/client/model/ModelRenderer", "(Lnet/minecraft/client/model/ModelBase;II)V",
        &[JValue::Object(base), JValue::Int(0), JValue::Int(0)])?;
    env.set_field(&r, "field_78800_c", "F", JValue::Float(p[0]))?;
    env.set_field(&r, "field_78797_d", "F", JValue::Float(p[1]))?;
    env.set_field(&r, "field_78798_e", "F", JValue::Float(p[2]))?;
    if mirror { env.set_field(&r, "field_78809_i", "Z", JValue::Bool(1))?; }
    Ok(r)
}

pub(crate) fn register_all() {
    pack_entry();
    pack_repository();
    particles();
    models();
    textures();
    renderers();
}

/// `ResourcePackRepository.Entry`: a pack file the repository was built with, and the pack instance cut from it.
fn pack_entry() {
    register(&format!("{RPR}.<init>"), |c| {
        let this = c.env.new_local_ref(&c.this)?;
        let (repo, file) = (c.obj(0)?, c.obj(1)?);
        c.env.set_field(&this, "field_110523_b", "Ljava/io/File;", JValue::Object(&file))?;
        // Synthetic outer link, absent from the SRG listing: without it only the metadata serializer is missing.
        if c.env.set_field(&this, "this$0", "Lnet/minecraft/client/resources/ResourcePackRepository;", JValue::Object(&repo)).is_err() {
            let _ = c.env.exception_clear();
        }
        Ok(Ret::Null)
    });

    // closeResourcePack: IOUtils.closeQuietly, so a throwing close is swallowed.
    register(&format!("{RPR}.func_110517_b"), |c| {
        let this = c.env.new_local_ref(&c.this)?;
        let pack = field(c.env, &this, "field_110524_c", "Lnet/minecraft/client/resources/IResourcePack;")?;
        if !pack.is_null() && c.env.is_instance_of(&pack, "java/io/Closeable")? {
            if c.env.call_method(&pack, "close", "()V", &[]).is_err() { let _ = c.env.exception_clear(); }
        }
        Ok(Ret::Null)
    });

    // getResourcePackName: the pack's own name; a pack cut from a file answers it, nothing else does.
    register(&format!("{RPR}.func_110515_d"), |c| {
        let this = c.env.new_local_ref(&c.this)?;
        let pack = field(c.env, &this, "field_110524_c", "Lnet/minecraft/client/resources/IResourcePack;")?;
        if pack.is_null() { return Ok(Ret::Null); }
        let name = call_opt(c.env, &pack, "func_130077_b", "()Ljava/lang/String;")?;
        if name.is_null() { return Ok(Ret::Null); }
        Ok(Ret::Obj(c.env.new_global_ref(&name)?))
    });

    // updateResourcePack: wrap the entry's file in a ResourcePackFile. Vanilla then reads pack.mcmeta through the
    // repository's serializer and closes the handle again; neither is reachable here (ResourcePackFile is a stub and
    // the serializer field is not mirrored), so the assignment is what a mod can see.
    register(&format!("{RPR}.func_110516_a"), |c| {
        let this = c.env.new_local_ref(&c.this)?;
        let file = field(c.env, &this, "field_110523_b", "Ljava/io/File;")?;
        if !file.is_null() {
            if let Ok(pack) = rt::new_object(c.env, "net/minecraft/client/resources/ResourcePackFile", "(Ljava/io/File;)V", &[JValue::Object(&file)]) {
                c.env.set_field(&this, "field_110524_c", "Lnet/minecraft/client/resources/IResourcePack;", JValue::Object(&pack))?;
            } else {
                let _ = c.env.exception_clear();
            }
        }
        Ok(Ret::Null)
    });

    // hashCode: name, "folder"/"zip" and mtime joined, hashed as vanilla's String.format(...).hashCode().
    register(&format!("{RPR}.hashCode"), |c| {
        let this = c.env.new_local_ref(&c.this)?;
        let file = field(c.env, &this, "field_110523_b", "Ljava/io/File;")?;
        if file.is_null() { return Ok(0.into()); }
        let name = c.env.call_method(&file, "getName", "()Ljava/lang/String;", &[])?.l()?;
        let name: String = if name.is_null() { String::new() } else { c.env.get_string(&name.into())?.into() };
        let kind = if c.env.call_method(&file, "isDirectory", "()Z", &[])?.z()? { "folder" } else { "zip" };
        let mtime = c.env.call_method(&file, "lastModified", "()J", &[])?.j()?;
        Ok(java_hash(&format!("{name}:{kind}:{mtime}")).into())
    });
}

/// `ResourcePackRepository`: the repository's lists and files. A repository we never built holds no packs, which is
/// what the vanilla getters answer for that state (an immutable empty list / a null instance).
fn pack_repository() {
    let lists = [
        ("net/minecraft/client/resources/ResourcePackRepository.func_110609_b", "field_110619_e"),
        ("net/minecraft/client/resources/ResourcePackRepository.func_110613_c", "field_110617_f"),
    ];
    for (key, name) in lists {
        register(key, move |c| {
            let this = c.env.new_local_ref(&c.this)?;
            let list = field(c.env, &this, name, "Ljava/util/List;")?;
            let out = if list.is_null() {
                rt::call_static(c.env, "com/google/common/collect/ImmutableList", "of",
                    "()Lcom/google/common/collect/ImmutableList;", &[])?
            } else {
                rt::call_static(c.env, "com/google/common/collect/ImmutableList", "copyOf",
                    "(Ljava/lang/Iterable;)Lcom/google/common/collect/ImmutableList;", &[JValue::Object(&list)])?
            };
            let out = out.l()?;
            Ok(Ret::Obj(c.env.new_global_ref(&out)?))
        });
    }

    let getters = [
        ("net/minecraft/client/resources/ResourcePackRepository.func_110612_e", "field_110618_d", "Ljava/io/File;"),
        ("net/minecraft/client/resources/ResourcePackRepository.func_148530_e", "field_148532_f", "Lnet/minecraft/client/resources/IResourcePack;"),
    ];
    for (key, name, sig) in getters {
        register(key, move |c| {
            let this = c.env.new_local_ref(&c.this)?;
            let v = field(c.env, &this, name, sig)?;
            if v.is_null() { return Ok(Ret::Null); }
            Ok(Ret::Obj(c.env.new_global_ref(&v)?))
        });
    }
}

/// `EffectRenderer` / `EntityFX`: the client owns the particle pass, so nothing is queued from here and light comes
/// from the client's own world copy.
fn particles() {
    // addBlockHitEffects: needs the block state, bounds and block texture of the hit block, none of which are
    // reachable through a stubbed world.
    register("net/minecraft/client/particle/EffectRenderer.func_180532_a", |_| Ok(Ret::Null));

    // getStatistics: "" + live effect count; effects live in the client, so this renderer holds none.
    register("net/minecraft/client/particle/EffectRenderer.func_78869_b", |_| Ok("0".into()));

    // renderParticle: writes one billboard quad into the caller's WorldRenderer; there is no particle batch.
    for key in ["net/minecraft/client/particle/EntityFX.func_180434_a", "*.func_180434_a"] {
        register(key, |_| Ok(Ret::Null));
    }

    // EntityFX.getBrightnessForRender: the packed light at the particle's own position (no eye offset).
    register("net/minecraft/client/particle/EntityFX.func_70070_b", |c| super::gap_entity::entity_light(c, true, false));
}

/// Baked model classes: the bake pass never runs, so only what a mod itself puts in these objects exists.
fn models() {
    // ItemModelMesher.getModelManager.
    register("net/minecraft/client/renderer/ItemModelMesher.func_178083_a", |c| {
        let this = c.env.new_local_ref(&c.this)?;
        let mm = field(c.env, &this, "field_178090_d", "Lnet/minecraft/client/resources/model/ModelManager;")?;
        if mm.is_null() { return Ok(Ret::Null); }
        Ok(Ret::Obj(c.env.new_global_ref(&mm)?))
    });

    // getItemModel: simpleShapesCache/shapers are only filled by ItemModelMesher.register (unimplemented), so
    // vanilla's two cache lookups always miss and the answer is the model manager's missing model.
    register("net/minecraft/client/renderer/ItemModelMesher.func_178089_a", |c| {
        let this = c.env.new_local_ref(&c.this)?;
        let mm = field(c.env, &this, "field_178090_d", "Lnet/minecraft/client/resources/model/ModelManager;")?;
        if mm.is_null() { return Ok(Ret::Null); }
        let m = call_opt(c.env, &mm, "func_174951_a", &format!("(){MODEL_SIG}"))?;
        if m.is_null() { return Ok(Ret::Null); }
        Ok(Ret::Obj(c.env.new_global_ref(&m)?))
    });

    // BakedQuad.getVertexData / getFace: plain field reads of data a mod or the (absent) baker wrote.
    let quads = [
        ("net/minecraft/client/renderer/block/model/BakedQuad.func_178209_a", "field_178215_a", "[I"),
        ("net/minecraft/client/renderer/block/model/BakedQuad.func_178210_d", "field_178214_c", "Lnet/minecraft/util/EnumFacing;"),
    ];
    for (key, name, sig) in quads {
        register(key, move |c| {
            let this = c.env.new_local_ref(&c.this)?;
            let v = field(c.env, &this, name, sig)?;
            if v.is_null() { return Ok(Ret::Null); }
            Ok(Ret::Obj(c.env.new_global_ref(&v)?))
        });
    }

    // ModelManager.getModel: the registry only holds what the loader registered, which is nothing here, so vanilla
    // falls through to the missing model for every location (and for a null one straight away).
    register("net/minecraft/client/resources/model/ModelManager.func_174953_a", |c| {
        let this = c.env.new_local_ref(&c.this)?;
        let v = field(c.env, &this, "field_174955_d", MODEL_SIG)?;
        if v.is_null() { return Ok(Ret::Null); }
        Ok(Ret::Obj(c.env.new_global_ref(&v)?))
    });
    register("net/minecraft/client/resources/model/ModelManager.func_174951_a", |c| {
        let this = c.env.new_local_ref(&c.this)?;
        let v = field(c.env, &this, "field_174955_d", MODEL_SIG)?;
        if v.is_null() { return Ok(Ret::Null); }
        Ok(Ret::Obj(c.env.new_global_ref(&v)?))
    });

    // SimpleBakedModel.Builder(IBakedModel, sprite): the private ctor's field initialisers plus vanilla's copy of the
    // wrapped model's general quads and particle texture. Vanilla re-uvs every quad through BreakingFour; sprite UV
    // interpolation is unimplemented, so the quads are carried over as they are.
    register(&format!("{BUILDER}.<init>"), |c| {
        let this = c.env.new_local_ref(&c.this)?;
        let model = c.obj(0)?;
        let general = rt::new_object(c.env, "java/util/ArrayList", "()V", &[])?;
        let face = rt::new_object(c.env, "java/util/ArrayList", "(I)V", &[JValue::Int(6)])?;
        for _ in 0..6 {
            let list = rt::new_object(c.env, "java/util/ArrayList", "()V", &[])?;
            c.env.call_method(&face, "add", "(Ljava/lang/Object;)Z", &[JValue::Object(&list)])?;
        }
        c.env.set_field(&this, "field_177656_a", "Ljava/util/List;", JValue::Object(&general))?;
        c.env.set_field(&this, "field_177654_b", "Ljava/util/List;", JValue::Object(&face))?;
        if !model.is_null() {
            let quads = call_opt(c.env, &model, "func_177550_a", "()Ljava/util/List;")?;
            if !quads.is_null() {
                c.env.call_method(&general, "addAll", "(Ljava/util/Collection;)Z", &[JValue::Object(&quads)])?;
            }
            let texture = call_opt(c.env, &model, "func_177554_e", &format!("(){SPRITE_SIG}"))?;
            if !texture.is_null() {
                c.env.set_field(&this, "field_177652_d", SPRITE_SIG, JValue::Object(&texture))?;
            }
            let ao = call_bool(c.env, &model, "func_177555_b");
            let gui3d = call_bool(c.env, &model, "func_177556_c");
            c.env.set_field(&this, "field_177655_c", "Z", JValue::Bool(ao as u8))?;
            c.env.set_field(&this, "field_177653_e", "Z", JValue::Bool(gui3d as u8))?;
            let ct = call_opt(c.env, &model, "func_177552_f", "()Lnet/minecraft/client/renderer/block/model/ItemCameraTransforms;")?;
            if !ct.is_null() {
                c.env.set_field(&this, "field_177651_f", "Lnet/minecraft/client/renderer/block/model/ItemCameraTransforms;", JValue::Object(&ct))?;
            }
        }
        Ok(Ret::Null)
    });

    // makeBakedModel: vanilla throws "Missing particle!" when the builder has no texture; a model without one is
    // returned as null here rather than as an exception.
    register(&format!("{BUILDER}.func_177645_b"), |c| {
        let this = c.env.new_local_ref(&c.this)?;
        let texture = field(c.env, &this, "field_177652_d", SPRITE_SIG)?;
        if texture.is_null() { return Ok(Ret::Null); }
        let general = field(c.env, &this, "field_177656_a", "Ljava/util/List;")?;
        let face = field(c.env, &this, "field_177654_b", "Ljava/util/List;")?;
        let ao = c.env.get_field(&this, "field_177655_c", "Z")?.z()?;
        let gui3d = c.env.get_field(&this, "field_177653_e", "Z")?.z()?;
        let ct = field(c.env, &this, "field_177651_f", "Lnet/minecraft/client/renderer/block/model/ItemCameraTransforms;")?;
        let model = rt::new_object(c.env, BAKED,
            "(Ljava/util/List;Ljava/util/List;ZZLnet/minecraft/client/renderer/texture/TextureAtlasSprite;Lnet/minecraft/client/renderer/block/model/ItemCameraTransforms;)V",
            &[JValue::Object(&general), JValue::Object(&face), JValue::Bool(ao as u8), JValue::Bool(gui3d as u8),
              JValue::Object(&texture), JValue::Object(&ct)])?;
        Ok(Ret::Obj(c.env.new_global_ref(&model)?))
    });

    // Forge's model loader is never started, so there is no registry to read a model from and no atlas function to
    // hand out; both answer like an unpopulated registry.
    register("net/minecraftforge/client/model/ModelLoaderRegistry.getModel", |_| Ok(Ret::Null));
    register("net/minecraftforge/client/model/ModelLoader.defaultTextureGetter", |_| Ok(Ret::Null));

    // BlockInfo's light arrays are filled by Forge's lighting pipeline (updateLightMatrix), which runs for quads the
    // client pipeline bakes; a BlockInfo nobody filled holds no values.
    register("net/minecraftforge/client/model/pipeline/BlockInfo.getBlockLight", |_| Ok(Ret::Null));
    register("net/minecraftforge/client/model/pipeline/BlockInfo.getSkyLight", |_| Ok(Ret::Null));
}

/// Texture atlas: sprites a mod registers or looks up in a TextureMap it owns.
fn textures() {
    register("net/minecraft/client/renderer/texture/TextureAtlasSprite.<init>", |c| {
        let this = c.env.new_local_ref(&c.this)?;
        let name = c.obj(0)?;
        c.env.set_field(&this, "field_110984_i", "Ljava/lang/String;", JValue::Object(&name))?;
        // The field initialiser that made the frame list does not run in a stub.
        let frames = rt::new_object(c.env, "java/util/ArrayList", "()V", &[])?;
        c.env.set_field(&this, "field_110976_a", "Ljava/util/List;", JValue::Object(&frames))?;
        Ok(Ret::Null)
    });

    // getFrameCount: an animated sprite's frame count, 0 for a sprite that never loaded one.
    register("net/minecraft/client/renderer/texture/TextureAtlasSprite.func_110970_k", |c| {
        let this = c.env.new_local_ref(&c.this)?;
        let frames = field(c.env, &this, "field_110976_a", "Ljava/util/List;")?;
        if frames.is_null() { return Ok(0.into()); }
        Ok(c.env.call_method(&frames, "size", "()I", &[])?.i()?.into())
    });

    // registerSprite: the map's own entry, else one cut by the map's icon creator and remembered there.
    register("net/minecraft/client/renderer/texture/TextureMap.func_174942_a", |c| {
        let this = c.env.new_local_ref(&c.this)?;
        let loc = c.obj(0)?;
        let map = field(c.env, &this, "field_110574_e", "Ljava/util/Map;")?;
        if loc.is_null() || map.is_null() { return Ok(Ret::Null); }
        let mut sprite = c.env.call_method(&map, "get", "(Ljava/lang/Object;)Ljava/lang/Object;", &[JValue::Object(&loc)])?.l()?;
        if sprite.is_null() {
            let creator = field(c.env, &this, "field_174946_m", "Lcom/google/common/base/Function;")?;
            if creator.is_null() { return Ok(Ret::Null); }
            sprite = c.env.call_method(&creator, "apply", "(Ljava/lang/Object;)Ljava/lang/Object;", &[JValue::Object(&loc)])?.l()?;
            if !sprite.is_null() {
                c.env.call_method(&map, "put", "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
                    &[JValue::Object(&loc), JValue::Object(&sprite)])?;
            }
        }
        if sprite.is_null() { return Ok(Ret::Null); }
        Ok(Ret::Obj(c.env.new_global_ref(&sprite)?))
    });

    // getAtlasSprite: the stitched sprite of that name, null before the atlas was stitched.
    register("net/minecraft/client/renderer/texture/TextureMap.func_110572_b", |c| {
        let this = c.env.new_local_ref(&c.this)?;
        let name = c.obj(0)?;
        let map = field(c.env, &this, "field_94252_e", "Ljava/util/Map;")?;
        if name.is_null() || map.is_null() { return Ok(Ret::Null); }
        let sprite = c.env.call_method(&map, "get", "(Ljava/lang/Object;)Ljava/lang/Object;", &[JValue::Object(&name)])?.l()?;
        if sprite.is_null() { return Ok(Ret::Null); }
        Ok(Ret::Obj(c.env.new_global_ref(&sprite)?))
    });

    // LayeredColorMaskTexture: base texture plus the mask lists; the pixels come from loadTexture, not the ctor.
    register("net/minecraft/client/renderer/texture/LayeredColorMaskTexture.<init>", |c| {
        let this = c.env.new_local_ref(&c.this)?;
        let (location, masks, colors) = (c.obj(0)?, c.obj(1)?, c.obj(2)?);
        c.env.set_field(&this, "field_174948_g", "Lnet/minecraft/util/ResourceLocation;", JValue::Object(&location))?;
        c.env.set_field(&this, "field_174949_h", "Ljava/util/List;", JValue::Object(&masks))?;
        c.env.set_field(&this, "field_174950_i", "Ljava/util/List;", JValue::Object(&colors))?;
        Ok(Ret::Null)
    });
}

/// Item/entity renderers and model classes. The client draws items, entities and nameplates in Rust; the vanilla
/// bodies all end in a GL/Tessellator pass that has no target here, so they consume the call and draw nothing.
fn renderers() {
    // ItemRenderer stays real bytecode (mods mixin into it), so its bodies are vanilla's own.
    // Render.doRender is empty in vanilla (subclasses draw); renderItem/renderName/render(model) need the entity
    // render pass and the baked model it would draw.
    for (class, names) in [
        ("net/minecraft/client/renderer/entity/Render", &["func_76986_a"][..]),
        ("net/minecraft/client/renderer/entity/RenderItem", &["func_181564_a"][..]),
        ("net/minecraft/client/renderer/entity/RendererLivingEntity", &["func_177067_a"][..]),
        ("net/minecraft/client/model/ModelBase", &["func_78088_a"][..]),
        ("net/minecraft/client/model/ModelPig", &["func_78088_a"][..]),
        ("net/minecraft/client/model/ModelSheep1", &["func_78088_a"][..]),
        ("net/minecraftforge/client/GuiIngameForge", &["func_73734_a"][..]),
    ] {
        for name in names {
            register(&format!("{class}.{name}"), |_| Ok(Ret::Null));
            register(&format!("*.{name}"), |_| Ok(Ret::Null));
        }
    }

    // Gui.drawRect is inherited, so a call site typed GuiIngameForge resolves to Gui's stub body.
    register("net/minecraft/client/gui/Gui.func_73734_a", |_| Ok(Ret::Null));
}
