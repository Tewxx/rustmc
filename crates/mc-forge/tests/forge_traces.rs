//! Runs the built tests/forge mods on the Rust-backed Forge API and writes their `ft.Trace` files to `$FT_TRACE_OUT`.
//! tools/forgetest/run-rustmc.sh builds the jars, sets the environment and diffs the output against tests/forge/oracle.
use mc_forge::{game, impls, runtime};
use std::path::{Path, PathBuf};

#[test]
fn forge_test_mods_produce_traces() {
    // The JVM runs Java on the calling thread: LaunchClassLoader plus class loading needs more than a test thread's 2 MiB.
    std::thread::Builder::new().stack_size(64 << 20).spawn(run).unwrap().join().unwrap();
}

/// The mods tests/forge/mods/ORDER lists, as built jars in `dir`.
fn ordered_mods(dir: &Path) -> Vec<PathBuf> {
    let order = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/forge/mods/ORDER");
    let names: Vec<String> = std::fs::read_to_string(order).unwrap_or_default()
        .lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(str::to_owned).collect();
    let mut jars: Vec<PathBuf> = names.iter().map(|n| dir.join(format!("{n}.jar"))).filter(|p| p.is_file()).collect();
    if jars.is_empty() {
        jars = std::fs::read_dir(dir).unwrap().filter_map(|e| Some(e.ok()?.path())).filter(|p| p.extension().is_some_and(|e| e == "jar")).collect();
        jars.sort();
    }
    jars
}

fn set_property(env: &mut jni::JNIEnv<'_>, key: &str, value: &str) {
    let (k, v) = (env.new_string(key).unwrap(), env.new_string(value).unwrap());
    env.call_static_method("java/lang/System", "setProperty", "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        &[jni::objects::JValue::Object(&k), jni::objects::JValue::Object(&v)]).unwrap();
}

fn run() {
    let (Some(stubs), Some(libs), Some(jvm)) = (std::env::var_os("RUSTMC_STUBS_JAR"), std::env::var_os("RUSTMC_FORGE_CP"), runtime::find_libjvm())
        else { eprintln!("skipped: no stubs jar, RUSTMC_FORGE_CP or JDK 8; run tools/forgetest/run-rustmc.sh"); return };
    let mods_dir = PathBuf::from(std::env::var_os("FT_MODS_DIR").expect("FT_MODS_DIR"));
    let out = std::fs::canonicalize(std::env::var_os("FT_TRACE_OUT").expect("FT_TRACE_OUT")).expect("FT_TRACE_OUT exists");
    let mods = ordered_mods(&mods_dir);
    assert!(!mods.is_empty(), "no built mods in {}", mods_dir.display());

    let stubs = std::fs::canonicalize(stubs).unwrap();
    let mut cp = vec![stubs];
    cp.extend(mods.iter().cloned());
    cp.extend(std::env::split_paths(&libs));
    runtime::start(&jvm, &cp, &mods).unwrap();
    runtime::with_env(|env| {
        impls::set_display((427, 240), (854, 480));
        impls::set_session("FTBot", "00000000000000000000000000000001");
        set_property(env, "ft.trace.dir", out.to_str().unwrap());
        // No world or player to tick: ftcommon's scripted driver would never advance, only time the session out.
        set_property(env, "rustmc.headlessProbe", "true");
    }).unwrap();

    let ids = runtime::load_mods(&mods).unwrap();
    eprintln!("loaded: {ids:?}");
    // Twenty game ticks and their frames, the points where the client fires mod events.
    runtime::with_env(|env| {
        for _ in 0..20 {
            game::client_tick(env, true).unwrap();
            game::render_tick(env, true, 0.5).unwrap();
            game::client_tick(env, false).unwrap();
            game::render_tick(env, false, 0.5).unwrap();
        }
    }).unwrap();

    let mut traces: Vec<String> = std::fs::read_dir(&out).unwrap()
        .filter_map(|e| Some(e.ok()?.file_name().to_string_lossy().into_owned())).collect();
    traces.sort();
    eprintln!("traces in {}: {traces:?}", out.display());
    assert!(!traces.is_empty(), "the mods wrote no trace files");
    eprintln!("still stubbed: {:#?}", mc_forge::stubs::stubbed());
}
