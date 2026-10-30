use jni::{InitArgsBuilder, JNIVersion, JavaVM};
use std::{env, error::Error, time::Instant};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args().collect();
    if !(4..=5).contains(&args.len()) {
        return Err("usage: jvm-spike LIBJVM CLASSPATH CLASS_WITH_STATIC_VALUE_METHOD [CALLS]".into());
    }
    let calls: u32 = args.get(4).map_or(Ok(100_000), |v| v.parse())?;
    if calls == 0 { return Err("CALLS must be positive".into()); }
    let classpath = format!("-Djava.class.path={}", args[2]);
    let options = InitArgsBuilder::new().version(JNIVersion::V8).option(&classpath).build()?;
    let vm = JavaVM::with_libjvm(options, || Ok(&args[1]))?;
    let mut env = vm.attach_current_thread()?;
    let class = env.find_class(&args[3])?;
    let method = env.get_static_method_id(&class, "value", "()I")?;
    let start = Instant::now();
    let mut value = 0;
    for _ in 0..calls {
        // The method ID and return type above were resolved for this exact class and signature.
        value = unsafe { env.call_static_method_unchecked(&class, method, jni::signature::ReturnType::Primitive(jni::signature::Primitive::Int), &[]) }?.i()?;
    }
    println!("value={value} calls={calls} ns_per_call={:.1}", start.elapsed().as_nanos() as f64 / calls as f64);
    Ok(())
}
