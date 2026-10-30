//! Forge/FML entry points: the event bus and the objects mods fetch it through.
use super::singleton;
use crate::stubs::{register, Ret};
use crate::runtime as rt;
use jni::{objects::{GlobalRef, JValue}};
use std::sync::OnceLock;

const BUS: &str = "net/minecraftforge/fml/common/eventhandler/EventBus";
static EVENT_BUS: OnceLock<GlobalRef> = OnceLock::new();
static FML: OnceLock<GlobalRef> = OnceLock::new();

pub fn register_all() {
    // Listener bookkeeping is plain Java (rustmc.forge.Bus); Rust posts game events into it (see `game`).
    register(&format!("{BUS}.register"), |c| { let o = c.obj(0)?; rt::call_static(c.env, "rustmc/forge/Bus", "register", "(Ljava/lang/Object;)V", &[JValue::Object(&o)])?; Ok(Ret::Null) });
    register(&format!("{BUS}.unregister"), |c| { let o = c.obj(0)?; rt::call_static(c.env, "rustmc/forge/Bus", "unregister", "(Ljava/lang/Object;)V", &[JValue::Object(&o)])?; Ok(Ret::Null) });
    register(&format!("{BUS}.post"), |c| {
        let e = c.obj(0)?;
        let cancelled = rt::call_static(c.env, "rustmc/forge/Bus", "post", "(Lnet/minecraftforge/fml/common/eventhandler/Event;)Z", &[JValue::Object(&e)])?.z()?;
        Ok(cancelled.into())
    });
    // Forge's EventSubscriptionTransformer would add these per event class at load time; here they read the annotations.
    for (name, java) in [("isCancelable", "cancelable"), ("hasResult", "hasResult")] {
        register(&format!("net/minecraftforge/fml/common/eventhandler/Event.{name}"), move |c| {
            let this = c.env.new_local_ref(&c.this)?;
            Ok(rt::call_static(c.env, "rustmc/forge/Bus", java, "(Lnet/minecraftforge/fml/common/eventhandler/Event;)Z", &[JValue::Object(&this)])?.z()?.into())
        });
    }
    // This is always the client; Mixin's FML platform agent asks.
    register("net/minecraftforge/fml/relauncher/FMLLaunchHandler.side", |c| {
        let side = rt::static_field(c.env, "net/minecraftforge/fml/relauncher/Side", "CLIENT", "Lnet/minecraftforge/fml/relauncher/Side;")?.l()?;
        Ok(Ret::Obj(c.env.new_global_ref(side)?))
    });
    // Mixin's FML platform agent reads and extends these lists; an empty, shared, mutable list is what a mod-less FML has.
    for name in ["getIgnoredMods", "getReparseableCoremods"] {
        register(&format!("net/minecraftforge/fml/relauncher/CoreModManager.{name}"), move |c| {
            static LISTS: std::sync::Mutex<Vec<(&str, GlobalRef)>> = std::sync::Mutex::new(Vec::new());
            let found = LISTS.lock().unwrap().iter().find(|(n, _)| *n == name).map(|(_, g)| g.clone());
            let list = match found {
                Some(g) => g,
                None => { let l = rt::new_object(c.env, "java/util/ArrayList", "()V", &[])?; let g = c.env.new_global_ref(l)?; LISTS.lock().unwrap().push((name, g.clone())); g }
            };
            Ok(Ret::Obj(list))
        });
    }
    // Forge's three static buses are one bus here.
    register("net/minecraftforge/common/MinecraftForge.<clinit>", |c| {
        let bus = singleton(c.env, &EVENT_BUS, BUS)?;
        let class = rt::class(c.env, "net/minecraftforge/common/MinecraftForge")?;
        for field in ["EVENT_BUS", "TERRAIN_GEN_BUS", "ORE_GEN_BUS"] {
            let id = c.env.get_static_field_id(&class, field, "Lnet/minecraftforge/fml/common/eventhandler/EventBus;")?;
            c.env.set_static_field(&class, id, JValue::Object(&bus))?;
        }
        Ok(Ret::Null)
    });
    register("net/minecraftforge/fml/common/FMLCommonHandler.instance", |c| {
        let o = singleton(c.env, &FML, "net/minecraftforge/fml/common/FMLCommonHandler")?;
        Ok(Ret::Obj(c.env.new_global_ref(o)?))
    });
    register("net/minecraftforge/fml/common/FMLCommonHandler.bus", |c| {
        let o = singleton(c.env, &EVENT_BUS, BUS)?;
        Ok(Ret::Obj(c.env.new_global_ref(o)?))
    });
}
