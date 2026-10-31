//! Synchronous continuations for Java methods whose bodies are owned by Rust.
//! Instrumentation/Mixin HEAD, cancellation and RETURN surround the real host body.
use crate::{runtime, stubs::{register, Ret}};
use jni::{errors::Result, objects::JObject, JNIEnv};
use std::{cell::Cell, panic::{catch_unwind, AssertUnwindSafe}};

struct Invocation<'a> { key: &'a str, body: &'a mut dyn FnMut(), ran: bool, panicked: bool }
thread_local! { static ACTIVE: Cell<*mut ()> = const { Cell::new(std::ptr::null_mut()) }; }

/// Only the synchronous JNI call on this thread can enter this continuation. The pointer
/// is restored before the borrowed body goes out of scope, including on unwinding.
fn scoped(key: &str, body: &mut dyn FnMut(), invoke: impl FnOnce()) -> (bool, bool) {
    let mut call = Invocation { key, body, ran: false, panicked: false };
    struct Reset(*mut ());
    impl Drop for Reset { fn drop(&mut self) { ACTIVE.with(|a| a.set(self.0)); } }
    let _reset = Reset(ACTIVE.with(|a| a.replace((&mut call as *mut Invocation<'_>).cast())));
    invoke();
    (call.ran, call.panicked)
}

fn dispatch(key: &str) {
    ACTIVE.with(|active| {
        let ptr = active.get();
        if ptr.is_null() { return; }
        // SAFETY: scoped() owns this stack value for the entire synchronous Java call.
        // TLS excludes other threads; `ran` prevents reentrant calls from aliasing body.
        let call = unsafe { &mut *ptr.cast::<Invocation<'_>>() };
        if call.key != key || call.ran { return; }
        call.ran = true;
        call.panicked = catch_unwind(AssertUnwindSafe(&mut *call.body)).is_err();
    });
}

/// False means Java instrumentation cancelled the method before reaching its body.
pub fn call(env: &mut JNIEnv<'_>, object: &JObject<'_>, method: &str, body: &mut dyn FnMut()) -> Result<bool> {
    let mut result = Ok(());
    let (ran, panicked) = scoped(method, body, || {
        result = env.call_method(object, method, "()V", &[]).map(drop);
    });
    result?;
    if panicked { return Err(jni::errors::Error::JavaException); }
    Ok(ran)
}

/// `call` for a method with arguments: `sig` is its JNI signature (returning void).
pub fn call_with(env: &mut JNIEnv<'_>, object: &JObject<'_>, method: &str, sig: &str, args: &[jni::objects::JValue<'_, '_>], body: &mut dyn FnMut()) -> Result<bool> {
    let mut result = Ok(());
    let (ran, panicked) = scoped(method, body, || {
        result = env.call_method(object, method, sig, args).map(drop);
    });
    result?;
    if panicked { return Err(jni::errors::Error::JavaException); }
    Ok(ran)
}

/// The `Minecraft` methods whose bodies Rust owns and whose Mixin hooks mods rely on (Meowtils drives its whole
/// event system from these): runTick, startGame, runGameLoop, loadWorld(WorldClient) and the three mouse clicks.
const MINECRAFT_HOOKS: [&str; 7] = ["func_71407_l", "func_71384_a", "func_71411_J", "func_71403_a", "func_147116_af", "func_147121_ag", "func_147112_ai"];

pub(crate) fn register_all() {
    for (class, method) in MINECRAFT_HOOKS.iter().map(|m| ("net/minecraft/client/Minecraft", *m))
        .chain([("net/minecraft/client/entity/EntityPlayerSP", "func_175161_p")]) {
        register(&format!("{class}.{method}"), move |_| { dispatch(method); Ok(Ret::Null) });
    }
    // Exposed through the Forge loader, never the JVM system-loader copy.
    register("net/minecraft/client/Minecraft.func_175606_aa", |c| {
        let mc = crate::impls::minecraft(c.env)?;
        let entity = c.env.get_field(mc, "field_71439_g", "Lnet/minecraft/client/entity/EntityPlayerSP;")?.l()?;
        Ok(Ret::Obj(c.env.new_global_ref(entity)?))
    });
    // Live.view keeps the mirrored RenderManager in Minecraft.renderManager.
    register("net/minecraft/client/Minecraft.func_175598_ae", |c| {
        let mc = crate::impls::minecraft(c.env)?;
        let rm = c.env.get_field(mc, "field_175616_W", "Lnet/minecraft/client/renderer/entity/RenderManager;")?.l()?;
        Ok(Ret::Obj(c.env.new_global_ref(rm)?))
    });
}

pub fn run_tick(body: &mut dyn FnMut()) -> Option<Result<bool>> {
    runtime::with_env(|env| { let mc = crate::impls::minecraft(env)?; call(env, &mc, "func_71407_l", body) })
}

fn minecraft_hook(method: &str, body: &mut dyn FnMut()) -> Option<Result<bool>> {
    runtime::with_env(|env| { let mc = crate::impls::minecraft(env)?; call(env, &mc, method, body) })
}

/// `Minecraft.startGame`, once after mods load: mods' HEAD/TAIL hooks initialise there.
pub fn start_game() -> Option<Result<bool>> { minecraft_hook("func_71384_a", &mut || {}) }

/// `Minecraft.runGameLoop` around one rendered frame.
pub fn game_loop(body: &mut dyn FnMut()) -> Option<Result<bool>> { minecraft_hook("func_71411_J", body) }

/// Which mouse action `Minecraft.clickMouse`/`rightClickMouse`/`middleClickMouse` performs.
#[derive(Clone, Copy, Debug)]
pub enum Click { Left, Right, Middle }

/// One click; false if a mod cancelled it, so the client must skip the action.
pub fn click(kind: Click, body: &mut dyn FnMut()) -> Option<Result<bool>> {
    minecraft_hook(match kind { Click::Left => "func_147116_af", Click::Right => "func_147121_ag", Click::Middle => "func_147112_ai" }, body)
}

/// `Minecraft.loadWorld(WorldClient)` with the live Java world (null when leaving), after `Live` changed it.
pub fn load_world(env: &mut JNIEnv<'_>) -> Result<bool> {
    let mc = crate::impls::minecraft(env)?;
    let world = env.get_field(&mc, "field_71441_e", "Lnet/minecraft/client/multiplayer/WorldClient;")?.l()?;
    call_with(env, &mc, "func_71403_a", "(Lnet/minecraft/client/multiplayer/WorldClient;)V", &[jni::objects::JValue::Object(&world)], &mut || {})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn continuation_is_once_only_nested_and_thread_scoped() {
        let mut count = 0;
        let result = scoped("tick", &mut || count += 1, || {
            dispatch("other");
            std::thread::spawn(|| dispatch("tick")).join().unwrap();
            let mut inner = 0;
            assert_eq!(scoped("walk", &mut || inner += 1, || { dispatch("walk"); dispatch("walk"); }), (true, false));
            assert_eq!(inner, 1);
            dispatch("tick"); dispatch("tick");
        });
        assert_eq!(result, (true, false)); assert_eq!(count, 1);
        dispatch("tick"); assert_eq!(count, 1);
        assert_eq!(scoped("cancelled", &mut || count += 1, || {}), (false, false));
    }
    #[test]
    fn panics_are_contained_at_the_ffi_boundary() {
        assert_eq!(scoped("tick", &mut || panic!("test"), || dispatch("tick")), (true, true));
        assert!(ACTIVE.with(|a| a.get().is_null()));
    }
}
