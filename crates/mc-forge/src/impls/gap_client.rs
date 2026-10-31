//! Forge API surface mods reach for Minecraft singleton, settings, EntityRenderer/RenderGlobal, controller and
//! network manager: methods the corpus scan found unimplemented. Vanilla fields are read under their SRG names
//! (`Live.java` fills the ones it mirrors); GL-only bodies are no-ops because the Rust layer replays draws.
use super::minecraft;
use crate::{runtime as rt, stubs::{register, Ret}};
use jni::{errors::Result, objects::{JObject, JString, JValue}, JNIEnv};

fn ret_obj(env: &mut JNIEnv<'_>, obj: &JObject<'_>) -> Result<Ret> { Ok(Ret::Obj(env.new_global_ref(obj)?)) }

/// `Tessellator.getInstance().getWorldRenderer().pos(x, y, z).endVertex()`, the tail of every vanilla line draw.
fn pos(env: &mut JNIEnv<'_>, wr: &JObject<'_>, p: [f64; 3]) -> Result<()> {
    env.call_method(wr, "func_181662_b", "(DDD)Lnet/minecraft/client/renderer/WorldRenderer;",
        &[JValue::Double(p[0]), JValue::Double(p[1]), JValue::Double(p[2])])?;
    env.call_method(wr, "func_181675_d", "()V", &[])?;
    Ok(())
}

/// `I18n.format(<field of obj>)`, the key text `KeyBinding.compareTo` sorts on.
fn i18n<'l>(env: &mut JNIEnv<'l>, obj: &JObject<'_>, field: &str, empty: &JObject<'_>) -> Result<JObject<'l>> {
    let key = env.get_field(obj, field, "Ljava/lang/String;")?.l()?;
    rt::call_static(env, "net/minecraft/client/resources/I18n", "func_135052_a",
        "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;", &[JValue::Object(&key), JValue::Object(empty)])?.l()
}

/// `String.compareTo` of the two localised values of `field`: vanilla compares category, then description.
fn compare_i18n(env: &mut JNIEnv<'_>, a: &JObject<'_>, b: &JObject<'_>, field: &str, empty: &JObject<'_>) -> Result<i32> {
    let (x, y) = (i18n(env, a, field, empty)?, i18n(env, b, field, empty)?);
    if x.is_null() || y.is_null() {
        return Ok((!x.is_null()) as i32 - (!y.is_null()) as i32);
    }
    Ok(env.call_method(&x, "compareTo", "(Ljava/lang/String;)I", &[JValue::Object(&y)])?.i()?)
}

pub(crate) fn register_all() {
    // --- RenderGlobal ---

    // displayListEntitiesDirty: the next frame rebuilds the chunk lists.
    register("net/minecraft/client/renderer/RenderGlobal.func_174979_m", |c| {
        c.env.set_field(&c.this, "field_147595_R", "Z", JValue::Bool(1))?;
        Ok(Ret::Null)
    });
    // A selection box is 12 wireframe edges: two strips along the top and bottom, then the four uprights.
    register("net/minecraft/client/renderer/RenderGlobal.func_181561_a", |c| {
        let bb = c.obj(0)?;
        if bb.is_null() { return Ok(Ret::Null); }
        let get = |env: &mut JNIEnv<'_>, f: &str| -> Result<f64> { env.get_field(&bb, f, "D")?.d() };
        let (min_x, min_y, min_z) = (get(c.env, "field_72340_a")?, get(c.env, "field_72338_b")?, get(c.env, "field_72339_c")?);
        let (max_x, max_y, max_z) = (get(c.env, "field_72336_d")?, get(c.env, "field_72337_e")?, get(c.env, "field_72334_f")?);
        let tess = rt::call_static(c.env, "net/minecraft/client/renderer/Tessellator", "func_178181_a",
            "()Lnet/minecraft/client/renderer/Tessellator;", &[])?.l()?;
        let wr = c.env.call_method(&tess, "func_178180_c", "()Lnet/minecraft/client/renderer/WorldRenderer;", &[])?.l()?;
        // GL_LINE_STRIP is mode 3, GL_LINES is 1; the stub's `begin` ignores the vertex format.
        let batch = |env: &mut JNIEnv<'_>, mode: i32, pts: &[[f64; 3]]| -> Result<()> {
            env.call_method(&wr, "func_181668_a", "(ILnet/minecraft/client/renderer/VertexFormat;)V",
                &[JValue::Int(mode), JValue::Object(&JObject::null())])?;
            for p in pts { pos(env, &wr, *p)?; }
            env.call_method(&tess, "func_78381_a", "()V", &[])?;
            Ok(())
        };
        batch(c.env, 3, &[[min_x, min_y, min_z], [max_x, min_y, min_z], [max_x, min_y, max_z], [min_x, min_y, max_z], [min_x, min_y, min_z]])?;
        batch(c.env, 3, &[[min_x, max_y, min_z], [max_x, max_y, min_z], [max_x, max_y, max_z], [min_x, max_y, max_z], [min_x, max_y, min_z]])?;
        batch(c.env, 1, &[[min_x, min_y, min_z], [min_x, max_y, min_z], [max_x, min_y, min_z], [max_x, max_y, min_z],
            [max_x, min_y, max_z], [max_x, max_y, max_z], [min_x, min_y, max_z], [min_x, max_y, max_z]])?;
        Ok(Ret::Null)
    });
    // "C: rendered/total D: distance, <dispatch info>": both counts and the "(s)" flag describe the chunk render
    // lists this client never compiles, so they are vanilla's numbers for an empty set; so is the dispatch info,
    // which is a Java-only counter (RenderDispatcher.getDebugInfo) that nothing here fills.
    // getDebugInfoRenders over what Live.renderGlobal mirrors: drawn sections / loaded sections and the distance.
    // renderChunksMany ("(s) ") is vanilla's default true; the dispatcher's queue info is not mirrored (left empty).
    register("net/minecraft/client/renderer/RenderGlobal.func_72735_c", |c| {
        let infos = c.env.get_field(&c.this, "field_72755_R", "Ljava/util/List;")?.l()?;
        let drawn = if infos.is_null() { 0 } else { c.env.call_method(&infos, "size", "()I", &[])?.i()? };
        let frustum = c.env.get_field(&c.this, "field_175008_n", "Lnet/minecraft/client/renderer/ViewFrustum;")?.l()?;
        let chunks = if frustum.is_null() { JObject::null() } else { c.env.get_field(&frustum, "field_178164_f", "[Lnet/minecraft/client/renderer/chunk/RenderChunk;")?.l()? };
        let total = if chunks.is_null() { 0 } else { c.env.get_array_length(&jni::objects::JObjectArray::from(chunks))? };
        let distance = c.env.get_field(&c.this, "field_72739_F", "I")?.i()?;
        Ok(Ret::Str(format!("C: {drawn}/{total} (s) D: {distance}, ")))
    });
    // "E: rendered/total, B: hidden, I: invisible": vanilla's own counters, all zero without a world.
    register("net/minecraft/client/renderer/RenderGlobal.func_72723_d", |c| {
        let total = c.env.get_field(&c.this, "field_72748_H", "I")?.i()?;
        let rendered = c.env.get_field(&c.this, "field_72749_I", "I")?.i()?;
        let hidden = c.env.get_field(&c.this, "field_72750_J", "I")?.i()?;
        Ok(Ret::Str(format!("E: {rendered}/{total}, B: {hidden}, I: {}", total - hidden - rendered)))
    });

    // --- Minecraft ---

    // The pack repository only exists once the client has built one, so the field is all there is.
    register("net/minecraft/client/Minecraft.func_110438_M", |c| {
        let repo = c.env.get_field(&c.this, "field_110448_aq", "Lnet/minecraft/client/resources/ResourcePackRepository;")?.l()?;
        ret_obj(c.env, &repo)
    });
    // Vanilla always has an ItemRenderer at this field; mods reach renderItemAndEffectIntoGUI through it, and the
    // stub's methods dispatch to `Render`. Build it once, like the other per-client singletons.
    register("net/minecraft/client/Minecraft.func_175597_ag", |c| {
        const SIG: &str = "Lnet/minecraft/client/renderer/ItemRenderer;";
        let cur = c.env.get_field(&c.this, "field_175620_Y", SIG)?.l()?;
        if !cur.is_null() { return ret_obj(c.env, &cur); }
        let made = rt::new_object(c.env, "net/minecraft/client/renderer/ItemRenderer", "(Lnet/minecraft/client/Minecraft;)V",
            &[JValue::Object(&c.this)])?;
        c.env.set_field(&c.this, "field_175620_Y", SIG, JValue::Object(&made))?;
        ret_obj(c.env, &made)
    });
    // No atlas exists on this side, so the field stays empty; see the report for the mirror this needs.
    register("net/minecraft/client/Minecraft.func_147117_R", |c| {
        let map = c.env.get_field(&c.this, "field_147128_au", "Lnet/minecraft/client/renderer/texture/TextureMap;")?.l()?;
        ret_obj(c.env, &map)
    });
    // isCallingFromMinecraftThread: stubs dispatch on the caller's own OS thread, so compare with the game thread
    // (the one that registered the handlers and runs the client loop). Mods' worker threads must see false.
    let game = std::thread::current().id();
    register("net/minecraft/client/Minecraft.func_152345_ab", move |_| Ok(Ret::Bool(std::thread::current().id() == game)));
    // Display.update plus the fullscreen resize check; the window and the swap are the Rust client's.
    register("net/minecraft/client/Minecraft.func_175601_h", |_| Ok(Ret::Null));
    // refreshResources / scheduleResourcesRefresh: the client reloads its pack stack on its next frame; the future
    // is already done, as vanilla's is once the scheduled task has run (callers only chain on it).
    register("net/minecraft/client/Minecraft.func_110436_a", |_| { super::client::push_reload_resources(); Ok(Ret::Null) });
    register("net/minecraft/client/Minecraft.func_175603_A", |c| {
        super::client::push_reload_resources();
        let future = rt::call_static(c.env, "com/google/common/util/concurrent/Futures", "immediateFuture",
            "(Ljava/lang/Object;)Lcom/google/common/util/concurrent/ListenableFuture;", &[JValue::Object(&JObject::null())])?.l()?;
        ret_obj(c.env, &future)
    });
    // Nothing starts an integrated server here; the field is only set if a mod does.
    register("net/minecraft/client/Minecraft.func_71401_C", |c| {
        let server = c.env.get_field(&c.this, "field_71437_Z", "Lnet/minecraft/server/integrated/IntegratedServer;")?.l()?;
        ret_obj(c.env, &server)
    });

    // --- EntityRenderer ---

    // Vanilla only swaps the spectator entity_outline group when OpenGlHelper.shadersSupported, which is false in
    // this stub API; the post chain the client does run is its own.
    register("net/minecraft/client/renderer/EntityRenderer.func_175066_a", |_| Ok(Ret::Null));
    // disableLightmap/enableLightmap only move the active texture unit and the matrix mode around.
    register("net/minecraft/client/renderer/EntityRenderer.func_175072_h", |_| Ok(Ret::Null));
    register("net/minecraft/client/renderer/EntityRenderer.func_180436_i", |_| Ok(Ret::Null));
    // The map renderer exists once the client constructs the world; nothing here builds one.
    register("net/minecraft/client/renderer/EntityRenderer.func_147701_i", |c| {
        let maps = c.env.get_field(&c.this, "field_147709_v", "Lnet/minecraft/client/gui/MapItemRenderer;")?.l()?;
        ret_obj(c.env, &maps)
    });
    // The ortho projection and modelview vanilla sets up for GUI overlays; draws from a mod are replayed by the
    // client's UI layer in screen units, so there is no GL state to prepare.
    register("net/minecraft/client/renderer/EntityRenderer.func_78478_c", |_| Ok(Ret::Null));

    // --- OpenGlHelper ---

    // vanilla: framebufferSupported && mc.gameSettings.fboEnable. This renderer draws straight to the window, so
    // both halves are false and mods take the non-FBO path.
    register("net/minecraft/client/renderer/OpenGlHelper.func_148822_b", |c| {
        let supported = rt::static_field(c.env, "net/minecraft/client/renderer/OpenGlHelper", "field_148823_f", "Z")?.z()?;
        if !supported { return Ok(Ret::Bool(false)); }
        let mc = minecraft::minecraft(c.env)?;
        let settings = c.env.get_field(&mc, "field_71474_y", "Lnet/minecraft/client/settings/GameSettings;")?.l()?;
        let enabled = !settings.is_null() && c.env.get_field(&settings, "field_148448_g", "Z")?.z()?;
        Ok(Ret::Bool(enabled))
    });
    // Texture-unit and blend state: the Rust renderer owns GL, so there is nothing to set.
    register("net/minecraft/client/renderer/OpenGlHelper.func_77475_a", |_| Ok(Ret::Null));
    register("net/minecraft/client/renderer/OpenGlHelper.func_148821_a", |_| Ok(Ret::Null));

    // --- NetworkManager ---

    // The socket is the Rust client's and no channel object is mirrored, so vanilla's null receiver means closed.
    register("net/minecraft/network/NetworkManager.func_150724_d", |c| {
        let channel = c.env.get_field(&c.this, "field_150746_k", "Lio/netty/channel/Channel;")?.l()?;
        if channel.is_null() { return Ok(Ret::Bool(false)); }
        Ok(Ret::Bool(c.env.call_method(&channel, "isOpen", "()Z", &[])?.z()?))
    });
    // Vanilla marks `disconnected` and calls onDisconnect on a closed channel; disconnects arrive here from the Rust
    // client through the ClientNetHandler path instead, and the channel field is never set.
    register("net/minecraft/network/NetworkManager.func_179293_l", |_| Ok(Ret::Null));
    // Flushes the outbound queue and ticks the listener: packets a mod sends leave at send time through Live.packet
    // and the client ticks the handler itself, so there is no queue here to pump.
    register("net/minecraft/network/NetworkManager.func_74428_b", |_| Ok(Ret::Null));

    // --- GameSettings ---

    // vanilla: no key code means not held; negatives are mouse buttons offset by 100, the rest keyboard keys.
    register("net/minecraft/client/settings/GameSettings.func_100015_a", |c| {
        let key = c.obj(0)?;
        if key.is_null() { return Ok(Ret::Bool(false)); }
        let code = c.env.get_field(&key, "field_74512_d", "I")?.i()?;
        let held = match code {
            0 => false,
            k if k < 0 => rt::call_static(c.env, "org/lwjgl/input/Mouse", "isButtonDown", "(I)Z", &[JValue::Int(k + 100)])?.z()?,
            k => rt::call_static(c.env, "org/lwjgl/input/Keyboard", "isKeyDown", "(I)Z", &[JValue::Int(k)])?.z()?,
        };
        Ok(Ret::Bool(held))
    });
    // The per-category mix from the settings map, 1.0 where the category has no entry (vanilla's default too).
    register("net/minecraft/client/settings/GameSettings.func_151438_a", |c| {
        let levels = c.env.get_field(&c.this, "field_151446_aD", "Ljava/util/Map;")?.l()?;
        if levels.is_null() { return Ok(Ret::Float(1.0)); }
        let category = c.obj(0)?;
        let level = c.env.call_method(&levels, "get", "(Ljava/lang/Object;)Ljava/lang/Object;", &[JValue::Object(&category)])?.l()?;
        if level.is_null() { return Ok(Ret::Float(1.0)); }
        Ok(Ret::Float(c.env.call_method(&level, "floatValue", "()F", &[])?.f()?))
    });

    // --- PlayerControllerMP ---

    // The mode the server set on this controller; not mirrored, so the field is null without one.
    register("net/minecraft/client/multiplayer/PlayerControllerMP.func_178889_l", |c| {
        let kind = c.env.get_field(&c.this, "field_78779_k", "Lnet/minecraft/world/WorldSettings$GameType;")?.l()?;
        ret_obj(c.env, &kind)
    });
    // vanilla: currentGameType.isSurvivalOrAdventure(); with no mode known, no HUD is the safe answer.
    register("net/minecraft/client/multiplayer/PlayerControllerMP.func_78755_b", |c| {
        let kind = c.env.get_field(&c.this, "field_78779_k", "Lnet/minecraft/world/WorldSettings$GameType;")?.l()?;
        if kind.is_null() { return Ok(Ret::Bool(false)); }
        Ok(Ret::Bool(c.env.call_method(&kind, "func_77144_e", "()Z", &[])?.z()?))
    });

    // --- MinecraftServer ---

    // Same single-threaded model as the client half of this test.
    register("net/minecraft/server/MinecraftServer.func_152345_ab", |_| Ok(Ret::Bool(true)));
    register("net/minecraft/server/MinecraftServer.func_71276_C", |c| {
        let server = rt::static_field(c.env, "net/minecraft/server/MinecraftServer", "field_71305_c",
            "Lnet/minecraft/server/MinecraftServer;")?.l()?;
        ret_obj(c.env, &server)
    });

    // --- ShaderGroup / KeyBinding / MinecraftForgeClient ---

    // The name the group was loaded under, handed back as stored.
    register("net/minecraft/client/shader/ShaderGroup.func_148022_b", |c| {
        let name = c.env.get_field(&c.this, "field_148034_c", "Ljava/lang/String;")?.l()?;
        if name.is_null() { return Ok(Ret::Null); }
        Ok(Ret::Str(c.env.get_string(&JString::from(name))?.into()))
    });
    // Orders key bindings by localised category, then localised description, as the controls list does.
    register("net/minecraft/client/settings/KeyBinding.compareTo", |c| {
        let other = c.obj(0)?;
        let empty = c.env.new_object_array(0, "java/lang/Object", JObject::null())?;
        let by_category = compare_i18n(c.env, &c.this, &other, "field_151471_f", &empty)?;
        if by_category != 0 { return Ok(Ret::Int(by_category)); }
        Ok(Ret::Int(compare_i18n(c.env, &c.this, &other, "field_74515_c", &empty)?))
    });
    // Forge's static render pass index; -1 is vanilla's "not inside a pass", which is what this client reports since
    // it never runs block/item render passes through this hook.
    register("net/minecraftforge/client/MinecraftForgeClient.getRenderPass", |c| {
        match rt::static_field(c.env, "net/minecraftforge/client/MinecraftForgeClient", "renderPass", "I") {
            Ok(pass) => Ok(Ret::Int(pass.i()?)),
            Err(_) => Ok(Ret::Int(-1)),
        }
    });
}
