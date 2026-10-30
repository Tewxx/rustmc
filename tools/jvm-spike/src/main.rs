use jni::{objects::JClass, sys::{jclass, jint, JNIEnv as RawJNIEnv}, InitArgsBuilder, JNIVersion, JavaVM, NativeMethod};
use std::sync::atomic::{AtomicU64, Ordering};
static NATIVE_CALLS: AtomicU64 = AtomicU64::new(0);
extern "system" fn native_offset(_env: *mut RawJNIEnv, _class: jclass) -> jint {
    NATIVE_CALLS.fetch_add(1, Ordering::Relaxed);
    1
}
use std::{env, error::Error, time::Instant};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args().collect();
    if !(4..=6).contains(&args.len()) {
        return Err("usage: jvm-spike LIBJVM CLASSPATH CLASS_WITH_STATIC_VALUE_METHOD [CALLS] [hooked|shadow|shadow-control]".into());
    }
    let calls: u32 = args.get(4).map_or(Ok(100_000), |v| v.parse())?;
    if calls == 0 { return Err("CALLS must be positive".into()); }
    let classpath = format!("-Djava.class.path={}", args[2]);
    let options = InitArgsBuilder::new().version(JNIVersion::V8).option(&classpath).build()?;
    let vm = JavaVM::with_libjvm(options, || Ok(&args[1]))?;
    let mut env = vm.attach_current_thread()?;
    let class = env.find_class(&args[3])?;
    let method = env.get_static_method_id(&class, "value", "()I")?;
    if args.get(5).is_some_and(|s| s == "shadow" || s == "shadow-control") {
        let hooked = args[5] == "shadow";
        env.call_static_method(&class, "value", "()I", &[])?;
        let target = JClass::from(env.get_static_field(&class, "target", "Ljava/lang/Class;")?.l()?);
        let tick_class = JClass::from(env.get_static_field(&class, "tickBridge", "Ljava/lang/Class;")?.l()?);
        let tick_bridge = mc_forge::ClientTickBridge::new(&mut env, tick_class)?;
        tick_bridge.post(&mut env, true)?;
        tick_bridge.post(&mut env, false)?;
        tick_bridge.post(&mut env, true)?;
        let event_bridge = JClass::from(env.get_static_field(&class, "eventBridge", "Ljava/lang/Class;")?.l()?);
        let movement_class = JClass::from(env.get_static_field(&class, "movementClass", "Ljava/lang/Class;")?.l()?);
        let movement_class2 = JClass::from(env.new_local_ref(&movement_class)?);
        let movement = mc_forge::MovementView::new(&mut env, movement_class)?;
        let other = mc_forge::MovementView::new(&mut env, movement_class2)?;
        if env.is_same_object(movement.object().as_obj(), other.object().as_obj())? {
            return Err("distinct movement owners share Java identity".into());
        }
        let identity = env.new_global_ref(movement.object().as_obj())?;
        let original = mc_forge::MovementState { strafe: 1.0, forward: -0.5, jump: true, sneak: false };
        movement.push(&mut env, original)?;
        if movement.pull(&mut env)? != original { return Err("movement state did not round-trip".into()); }
        env.call_static_method(&event_bridge, "editMovement", "(Lnet/minecraft/util/MovementInput;)V",
            &[jni::objects::JValue::Object(movement.object().as_obj())])?;
        let changed = mc_forge::MovementState { strafe: 1.5, forward: 0.5, jump: false, sneak: true };
        if movement.pull(&mut env)? != changed { return Err("Java movement edits did not reach Rust".into()); }
        movement.push(&mut env, original)?;
        if movement.pull(&mut env)? != original { return Err("Rust movement overwrite failed".into()); }
        if !env.is_same_object(identity.as_obj(), movement.object().as_obj())? {
            return Err("movement identity changed during synchronization".into());
        }
        let replacement_class = env.get_object_class(movement.object().as_obj())?;
        drop(movement);
        let replacement = mc_forge::MovementView::new(&mut env, replacement_class)?;
        if env.is_same_object(identity.as_obj(), replacement.object().as_obj())? {
            return Err("replacement movement owner reused stale Java identity".into());
        }
        println!("rust_forge_movement_view=passed");
        let phases = env.call_static_method(&event_bridge, "tickPhases", "()Ljava/lang/String;", &[])?.l()?;
        let phases = jni::objects::JString::from(phases);
        let phases: String = env.get_string(&phases)?.into();
        if phases != "SES" { return Err(format!("Forge client tick phases {phases}, expected SES").into()); }
        println!("rust_forge_tick_bridge=passed");
        for (input, expected) in [(2, 170), (-2, 71)] {
            let result = env.call_static_method(&event_bridge, "postBridgeEvent", "(I)I", &[jni::objects::JValue::Int(input)])?.i()?;
            if result != expected { return Err(format!("Forge event bridge returned {result}, expected {expected} for {input}").into()); }
        }
        println!("rust_forge_event_bridge=passed");
        let bridge = JClass::from(env.get_static_field(&class, "bridge", "Ljava/lang/Class;")?.l()?);
        env.register_native_methods(&bridge, &[NativeMethod { name: "offset".into(), sig: "()I".into(), fn_ptr: native_offset as *mut _ }])?;
        let method = env.get_static_method_id(&target, "func_76128_c", "(D)I")?;
        let arguments = [jni::objects::JValue::Double(1.75).as_jni()];
        for _ in 0..10_000 {
            // Warm the JVM before measuring this exact method and callback path.
            unsafe { env.call_static_method_unchecked(&target, method, jni::signature::ReturnType::Primitive(jni::signature::Primitive::Int), &arguments) }?.i()?;
        }
        NATIVE_CALLS.store(0, Ordering::Relaxed);
        let start = Instant::now();
        let mut result = 0;
        for _ in 0..calls {
            // The method ID and signature were resolved from the transformed shadow class.
            result = unsafe { env.call_static_method_unchecked(&target, method, jni::signature::ReturnType::Primitive(jni::signature::Primitive::Int), &arguments) }?.i()?;
        }
        let native_calls = NATIVE_CALLS.load(Ordering::Relaxed);
        let expected = if hooked { 2 } else { 1 };
        let expected_calls = if hooked { u64::from(calls) } else { 0 };
        if result != expected || native_calls != expected_calls {
            return Err(format!("unexpected result {result}, Rust callbacks {native_calls}/{expected_calls}").into());
        }
        println!("shadow_mode={} value={result} rust_callbacks={native_calls} calls={calls} ns_per_dispatch={:.1}", if hooked { "hooked" } else { "control" }, start.elapsed().as_nanos() as f64 / calls as f64);
        return Ok(());
    }
    if args.get(5).is_some_and(|s| s == "hooked") {
        env.call_static_method(&class, "value", "()I", &[])?;
        let target = env.get_static_field(&class, "target", "Ljava/lang/Object;")?.l()?;
        let target_class = env.get_object_class(&target)?;
        let method = env.get_method_id(&target_class, "value", "()I")?;
        let start = Instant::now();
        let mut result = 0;
        for _ in 0..calls {
            // The method ID is resolved from this target's class and has signature ()I.
            result = unsafe { env.call_method_unchecked(&target, method, jni::signature::ReturnType::Primitive(jni::signature::Primitive::Int), &[]) }?.i()?;
        }
        if result != 2 { return Err(format!("unexpected transformed result: {result}").into()); }
        println!("hooked_value={result} calls={calls} ns_per_call={:.1}", start.elapsed().as_nanos() as f64 / calls as f64);
        return Ok(());
    }
    let start = Instant::now();
    let mut value = 0;
    for _ in 0..calls {
        // The method ID and return type above were resolved for this exact class and signature.
        value = unsafe { env.call_static_method_unchecked(&class, method, jni::signature::ReturnType::Primitive(jni::signature::Primitive::Int), &[]) }?.i()?;
    }
    println!("value={value} calls={calls} ns_per_call={:.1}", start.elapsed().as_nanos() as f64 / calls as f64);
    Ok(())
}
