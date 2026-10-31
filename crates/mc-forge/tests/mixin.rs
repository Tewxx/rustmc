//! Mixin applied through LaunchWrapper to a stubbed vanilla class. Run via tools/shadow/test-mods.sh.
use jni::objects::JValue;
use mc_forge::runtime;

#[test]
fn mixin_wraps_rust_handled_and_stubbed_methods() {
    // Java runs on the calling thread; LaunchClassLoader and Mixin need a deep stack.
    std::thread::Builder::new().stack_size(64 << 20).spawn(flow).unwrap().join().unwrap();
}

fn flow() {
    let (Some(stubs), Some(libs), Some(jvm), Some(mod_jar)) = (std::env::var_os("RUSTMC_STUBS_JAR"), std::env::var_os("RUSTMC_FORGE_CP"),
        runtime::find_libjvm(), std::env::var_os("RUSTMC_MIXIN_MOD_JAR")) else { eprintln!("skipped: run tools/shadow/test-mods.sh"); return };
    std::env::set_current_dir(std::env::temp_dir()).unwrap();
    let mod_jar = std::fs::canonicalize(mod_jar).unwrap();
    let mut cp = vec![std::fs::canonicalize(stubs).unwrap(), mod_jar.clone()];
    cp.extend(std::env::split_paths(&libs));
    runtime::start(&jvm, &cp, &[mod_jar]).unwrap();

    runtime::with_env(|env| {
        let math = "net/minecraft/util/MathHelper";
        // func_76128_c is implemented in Rust (floor); the mixin's RETURN injection sees its result and adds 100.
        let floor = runtime::call_static(env, math, "func_76128_c", "(D)I", &[JValue::Double(-1.5)]).unwrap().i().unwrap();
        assert_eq!(floor, -2 + 100);
        // func_76135_e (abs) is Rust-implemented too, but the HEAD injection answers before it runs.
        let abs = runtime::call_static(env, math, "func_76135_e", "(F)F", &[JValue::Float(-3.0)]).unwrap().f().unwrap();
        assert_eq!(abs, -1.0);
        // An untouched method keeps working.
        let ceil = runtime::call_static(env, math, "func_76123_f", "(F)I", &[JValue::Float(1.2)]).unwrap().i().unwrap();
        assert_eq!(ceil, 2);
    }).unwrap();
}
