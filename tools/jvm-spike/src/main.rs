use jni::{InitArgsBuilder, JNIVersion, JavaVM};
use std::{env, error::Error, time::Instant};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args().collect();
    if !(4..=6).contains(&args.len()) {
        return Err("usage: jvm-spike LIBJVM CLASSPATH CLASS_WITH_STATIC_VALUE_METHOD [CALLS] [hooked]".into());
    }
    let calls: u32 = args.get(4).map_or(Ok(100_000), |v| v.parse())?;
    if calls == 0 { return Err("CALLS must be positive".into()); }
    let classpath = format!("-Djava.class.path={}", args[2]);
    let options = InitArgsBuilder::new().version(JNIVersion::V8).option(&classpath).build()?;
    let vm = JavaVM::with_libjvm(options, || Ok(&args[1]))?;
    let mut env = vm.attach_current_thread()?;
    let class = env.find_class(&args[3])?;
    let method = env.get_static_method_id(&class, "value", "()I")?;
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
