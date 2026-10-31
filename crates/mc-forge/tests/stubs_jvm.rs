//! Needs `tools/shadow/stubs.sh tests/forge/build/stubs.jar` and a JDK 8; skips otherwise. One JVM per process.
use jni::objects::JValue;
use mc_forge::{runtime, stubs};
use std::path::PathBuf;

#[test]
fn rust_handlers_answer_stubbed_java_calls() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let jar = std::env::var_os("RUSTMC_STUBS_JAR").map_or(root.join("tests/forge/build/stubs.jar"), PathBuf::from);
    let jvm = runtime::find_libjvm().or_else(|| Some(root.join(".mc-tools/jdk8/Contents/Home/jre/lib/server/libjvm.dylib")).filter(|p| p.is_file()))
        .or_else(|| Some(root.join(".mc-tools/jdk8/jre/lib/amd64/server/libjvm.so")).filter(|p| p.is_file()));
    let (true, Some(jvm)) = (jar.is_file(), jvm) else { eprintln!("skipped: stubs jar or JDK 8 missing"); return };
    let jar = std::fs::canonicalize(jar).unwrap();
    std::env::set_current_dir(std::env::temp_dir()).unwrap();
    // LaunchWrapper, Mixin and Minecraft's libraries come from RUSTMC_FORGE_CP (tools/shadow/test-mods.sh sets it).
    let Some(libs) = std::env::var_os("RUSTMC_FORGE_CP") else { eprintln!("skipped: RUSTMC_FORGE_CP unset"); return };
    let libs = Some(libs);
    let mut cp = vec![jar];
    cp.extend(libs.iter().flat_map(std::env::split_paths));
    runtime::start(&jvm, &cp, &[]).unwrap();

    runtime::with_env(|env| {
        let math = "net/minecraft/util/MathHelper";
        let floor = runtime::call_static(env, math, "func_76128_c", "(D)I", &[JValue::Double(-1.5)]).unwrap().i().unwrap();
        assert_eq!(floor, -2);
        let sin = runtime::call_static(env, math, "func_76126_a", "(F)F", &[JValue::Float(1.0)]).unwrap().f().unwrap();
        assert_eq!(sin, mc_core::math::sin(1.0));
        // Not implemented in Rust: zero comes back and the call is recorded.
        let unimplemented = runtime::call_static(env, math, "func_82715_a", "(Ljava/lang/String;I)I", &[JValue::Object(&env.new_string("5").unwrap()), JValue::Int(3)]).unwrap().i().unwrap();
        assert_eq!(unimplemented, 0);
        if libs.is_none() { return; }
        let mc = "net/minecraft/client/Minecraft";
        let sig = "()Lnet/minecraft/client/Minecraft;";
        let a = runtime::call_static(env, mc, "func_71410_x", sig, &[]).unwrap().l().unwrap();
        let b = runtime::call_static(env, mc, "func_71410_x", sig, &[]).unwrap().l().unwrap();
        assert!(!a.is_null() && env.is_same_object(&a, &b).unwrap());
    }).unwrap();
    assert!(stubs::stubbed().iter().any(|k| k.starts_with("net/minecraft/util/MathHelper.func_82715_a")));
}
