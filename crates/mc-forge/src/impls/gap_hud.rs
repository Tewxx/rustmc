//! Forge API surface mods reach for HUD, chat, scoreboard and tab list: methods the corpus scan found unimplemented.
//! Rendering itself belongs to the Rust client, so the vanilla render passes are no-ops; the getters read the same
//! SRG fields `Live.java` mirrors.
use crate::{runtime as rt, stubs::{register, Ret}};
use jni::{errors::Result, objects::{JObject, JObjectArray, JValue}, JNIEnv};

const MC: &str = "Lnet/minecraft/client/Minecraft;";
const FONT: &str = "Lnet/minecraft/client/gui/FontRenderer;";
const CHAT: &str = "Lnet/minecraft/client/gui/GuiNewChat;";
const GAME: &str = "Lnet/minecraft/client/gui/GuiIngame;";
const TAB: &str = "Lnet/minecraft/client/gui/GuiPlayerTabOverlay;";
const BOARD: &str = "Lnet/minecraft/scoreboard/Scoreboard;";
const OBJECTIVE: &str = "Lnet/minecraft/scoreboard/ScoreObjective;";
const RESOURCE: &str = "Lnet/minecraft/util/ResourceLocation;";

/// Field read that keeps the object out of the `JValue` dance at every call site.
fn field<'l>(env: &mut JNIEnv<'l>, obj: &JObject<'_>, name: &str, sig: &str) -> Result<JObject<'l>> {
    Ok(env.get_field(obj, name, sig)?.l()?)
}

fn global<'l>(env: &mut JNIEnv<'l>, obj: JObject<'l>) -> Result<Ret> {
    Ok(Ret::Obj(env.new_global_ref(obj)?))
}

/// `new GuiPlayerTabOverlay(mc, guiIngame)` without running the stub constructor, as elsewhere.
fn tab_list<'l>(env: &mut JNIEnv<'l>, mc: &JObject<'_>, game: &JObject<'_>) -> Result<JObject<'l>> {
    let tab = rt::alloc(env, "net/minecraft/client/gui/GuiPlayerTabOverlay")?;
    env.set_field(&tab, "field_175250_f", MC, JValue::Object(mc))?;
    env.set_field(&tab, "field_175251_g", GAME, JValue::Object(game))?;
    Ok(tab)
}

pub(crate) fn register_all() {
    // Scoreboard: only teams are mirrored, so the display-slot array is the board object's own field.
    register("net/minecraft/scoreboard/Scoreboard.func_96539_a", |c| {
        let slots = field(c.env, &c.this, "field_96541_d", "[Lnet/minecraft/scoreboard/ScoreObjective;")?;
        let slot = c.i32(0)?;
        if slots.is_null() { return Ok(Ret::Null); } // no objective was ever displayed
        let slots = unsafe { JObjectArray::from_raw(slots.into_raw()) };
        if slot < 0 || slot >= c.env.get_array_length(&slots)? { return Ok(Ret::Null); }
        let objective = c.env.get_object_array_element(&slots, slot)?;
        if objective.is_null() { return Ok(Ret::Null); }
        global(c.env, objective)
    });
    // every tracked entity's scores, flattened like `Lists.newArrayList(Iterables.concat(...))`.
    register("net/minecraft/scoreboard/Scoreboard.func_96528_e", |c| {
        let maps = field(c.env, &c.this, "field_96544_c", "Ljava/util/Map;")?;
        let all = rt::new_object(c.env, "java/util/ArrayList", "()V", &[])?;
        if !maps.is_null() {
            let per_entity = c.env.call_method(&maps, "values", "()Ljava/util/Collection;", &[])?.l()?;
            let it = c.env.call_method(&per_entity, "iterator", "()Ljava/util/Iterator;", &[])?.l()?;
            while c.env.call_method(&it, "hasNext", "()Z", &[])?.z()? {
                let map = c.env.call_method(&it, "next", "()Ljava/lang/Object;", &[])?.l()?;
                let scores = c.env.call_method(&map, "values", "()Ljava/util/Collection;", &[])?.l()?;
                c.env.call_method(&all, "addAll", "(Ljava/util/Collection;)Z", &[JValue::Object(&scores)])?;
            }
        }
        global(c.env, all)
    });
    // `getValueFromObjective`: cache a Score per (name, objective), creating the maps on first use.
    register("net/minecraft/scoreboard/Scoreboard.func_96529_a", |c| {
        let name = c.obj(0)?;
        let objective = c.obj(1)?;
        if c.env.call_method(&name, "length", "()I", &[])?.i()? > 40 {
            c.env.throw_new("java/lang/IllegalArgumentException", "The player name is too long!")?;
            return Ok(Ret::Null);
        }
        let mut maps = field(c.env, &c.this, "field_96544_c", "Ljava/util/Map;")?;
        if maps.is_null() {
            maps = rt::new_object(c.env, "java/util/HashMap", "()V", &[])?;
            c.env.set_field(&c.this, "field_96544_c", "Ljava/util/Map;", JValue::Object(&maps))?;
        }
        let mut by_objective = c.env.call_method(&maps, "get", "(Ljava/lang/Object;)Ljava/lang/Object;",
            &[JValue::Object(&name)])?.l()?;
        if by_objective.is_null() {
            by_objective = rt::new_object(c.env, "java/util/HashMap", "()V", &[])?;
            c.env.call_method(&maps, "put", "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
                &[JValue::Object(&name), JValue::Object(&by_objective)])?;
        }
        let existing = c.env.call_method(&by_objective, "get", "(Ljava/lang/Object;)Ljava/lang/Object;",
            &[JValue::Object(&objective)])?.l()?;
        if !existing.is_null() { return global(c.env, existing); }
        let score = rt::alloc(c.env, "net/minecraft/scoreboard/Score")?;
        c.env.set_field(&score, "field_96656_b", BOARD, JValue::Object(&c.this))?;
        c.env.set_field(&score, "field_96657_c", OBJECTIVE, JValue::Object(&objective))?;
        c.env.set_field(&score, "field_96654_d", "Ljava/lang/String;", JValue::Object(&name))?;
        c.env.set_field(&score, "field_178818_g", "Z", JValue::Bool(1))?; // forceUpdate = true
        c.env.call_method(&by_objective, "put", "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
            &[JValue::Object(&objective), JValue::Object(&score)])?;
        global(c.env, score)
    });

    // `new GuiIngame(mc)`: the chat GUI shared with `getChatGUI` and the tab list, plus the title times and
    // `recordPlaying`. overlayDebug/spectatorGui/streamIndicator and itemRenderer have nothing to mirror yet.
    register("net/minecraft/client/gui/GuiIngame.<init>", |c| {
        let mc = c.obj(0)?;
        c.env.set_field(&c.this, "field_73839_d", MC, JValue::Object(&mc))?;
        c.env.set_field(&c.this, "field_175199_z", "I", JValue::Int(10))?;
        c.env.set_field(&c.this, "field_175192_A", "I", JValue::Int(70))?;
        c.env.set_field(&c.this, "field_175193_B", "I", JValue::Int(20))?;
        let empty = c.env.new_string("")?;
        c.env.set_field(&c.this, "field_73838_g", "Ljava/lang/String;", JValue::Object(&empty))?;
        let chat = c.env.call_method(&c.this, "func_146158_b", "()Lnet/minecraft/client/gui/GuiNewChat;", &[])?.l()?;
        c.env.set_field(&c.this, "field_73840_e", CHAT, JValue::Object(&chat))?;
        let tab = tab_list(c.env, &mc, &c.this)?;
        c.env.set_field(&c.this, "field_175196_v", TAB, JValue::Object(&tab))?;
        Ok(Ret::Null)
    });
    // The HUD is drawn by the Rust client (`RenderGameOverlayEvent` comes from there), so this pass draws nothing.
    register("net/minecraft/client/gui/GuiIngame.func_175180_a", |_| Ok(Ret::Null));
    register("net/minecraft/client/gui/GuiIngame.func_175181_h", |c| {
        let tab = field(c.env, &c.this, "field_175196_v", TAB)?;
        if !tab.is_null() { return global(c.env, tab); }
        let mc = field(c.env, &c.this, "field_73839_d", MC)?;
        let tab = tab_list(c.env, &mc, &c.this)?;
        c.env.set_field(&c.this, "field_175196_v", TAB, JValue::Object(&tab))?;
        global(c.env, tab)
    });

    // `GuiScreen.setWorldAndResolution` as GuiMultiplayer sees it: lay the screen out and rebuild its buttons.
    // itemRender is left alone (`Minecraft.getRenderItem` is not mirrored); fontRendererObj is copied from mc.
    register("net/minecraft/client/gui/GuiMultiplayer.func_146280_a", |c| {
        let mc = c.obj(0)?;
        let (width, height) = (c.i32(1)?, c.i32(2)?);
        c.env.set_field(&c.this, "field_146297_k", MC, JValue::Object(&mc))?;
        let font = field(c.env, &mc, "field_71466_p", FONT)?;
        c.env.set_field(&c.this, "field_146289_q", FONT, JValue::Object(&font))?;
        c.env.set_field(&c.this, "field_146294_l", "I", JValue::Int(width))?;
        c.env.set_field(&c.this, "field_146295_m", "I", JValue::Int(height))?;
        let buttons = field(c.env, &c.this, "field_146292_n", "Ljava/util/List;")?;
        if !buttons.is_null() { c.env.call_method(&buttons, "clear", "()V", &[])?; }
        c.env.call_method(&c.this, "func_73866_w_", "()V", &[])?;
        Ok(Ret::Null)
    });
    // The server list (and the delete/add/edit flags `confirmClicked` switches on) lives in Rust: a GuiMultiplayer
    // here has no entries, so connecting to a selection or answering its prompt does nothing.
    register("net/minecraft/client/gui/GuiMultiplayer.func_146796_h", |_| Ok(Ret::Null));
    register("net/minecraft/client/gui/GuiMultiplayer.func_73878_a", |_| Ok(Ret::Null));

    // Skins and capes are downloaded and cached by the Rust client; nothing mirrors them onto NetworkPlayerInfo yet.
    register("net/minecraft/client/network/NetworkPlayerInfo.func_178837_g", |c| {
        let skin = field(c.env, &c.this, "field_178865_e", RESOURCE)?;
        if skin.is_null() { return Ok(Ret::Null); }
        global(c.env, skin)
    });
    register("net/minecraft/client/network/NetworkPlayerInfo.func_178861_h", |c| {
        let cape = field(c.env, &c.this, "field_178862_f", RESOURCE)?;
        if cape.is_null() { return Ok(Ret::Null); }
        global(c.env, cape)
    });

    register("net/minecraft/client/gui/ChatLine.func_74539_c",
        |c| Ok(Ret::Int(c.env.get_field(&c.this, "field_74542_c", "I")?.i()?)));
    register("net/minecraft/client/gui/ChatLine.func_74540_b",
        |c| Ok(Ret::Int(c.env.get_field(&c.this, "field_74543_a", "I")?.i()?)));

    // `updatePlayerList`: remember when the list became visible, for its fade-in animation.
    register("net/minecraft/client/gui/GuiPlayerTabOverlay.func_175246_a", |c| {
        let rendered = c.bool(0)?;
        let was = c.env.get_field(&c.this, "field_175254_k", "Z")?.z()?;
        if rendered && !was {
            let now = rt::call_static(c.env, "net/minecraft/client/Minecraft", "func_71386_F", "()J", &[])?.j()?;
            c.env.set_field(&c.this, "field_175253_j", "J", JValue::Long(now))?;
        }
        c.env.set_field(&c.this, "field_175254_k", "Z", JValue::Bool(rendered.into()))?;
        Ok(Ret::Null)
    });
    // The tab list itself is drawn by the Rust client.
    register("net/minecraft/client/gui/GuiPlayerTabOverlay.func_175249_a", |_| Ok(Ret::Null));
    // Chat lines are Rust-side (`ChatLine` objects are minted per printed message, never kept in Java), so no line
    // can be under the mouse.
    register("net/minecraft/client/gui/GuiNewChat.func_146236_a", |_| Ok(Ret::Null));
}
