//! Loads every class a real mod's mixins target and reports which mixins applied. Needs RUSTMC_CORPUS_JAR and
//! RUSTMC_MIXIN_TARGETS (comma separated); `tools/shadow/corpus-mixins.sh` sets them. Prints a report, never fails on
//! an unapplied mixin: the list of failures is the to-do list for real bodies in `tools/shadow/keep-real.txt`.
use mc_forge::runtime;

#[test]
fn report_mixin_application_for_corpus_mod() {
    std::thread::Builder::new().stack_size(128 << 20).spawn(flow).unwrap().join().unwrap();
}

fn flow() {
    let (Some(stubs), Some(libs), Some(jvm), Some(jar), Ok(targets)) = (std::env::var_os("RUSTMC_STUBS_JAR"), std::env::var_os("RUSTMC_FORGE_CP"),
        runtime::find_libjvm(), std::env::var_os("RUSTMC_CORPUS_JAR"), std::env::var("RUSTMC_MIXIN_TARGETS")) else { eprintln!("skipped: run tools/shadow/corpus-mixins.sh"); return };
    std::env::set_current_dir(std::env::temp_dir()).unwrap();
    let jar = std::fs::canonicalize(jar).unwrap();
    let mut cp = vec![std::fs::canonicalize(stubs).unwrap(), jar.clone()];
    cp.extend(std::env::split_paths(&libs));
    runtime::start(&jvm, &cp, &[jar]).unwrap();
    runtime::with_env(|env| {
        let (mut ok, mut failed) = (0, Vec::new());
        for target in targets.split(',').filter(|t| !t.is_empty()) {
            let name = target.replace('.', "/");
            match runtime::class(env, &name) {
                Ok(_) => ok += 1,
                Err(_) => {
                    // LaunchClassLoader wraps the transformer failure in a ClassNotFoundException: report the root cause.
                    let mut why = String::new();
                    if let Some(mut e) = env.exception_occurred().ok().filter(|e| !e.is_null()) {
                        let _ = env.exception_clear();
                        for _ in 0..8 {
                            let text = env.call_method(&e, "toString", "()Ljava/lang/String;", &[]).ok().and_then(|s| s.l().ok())
                                .and_then(|s| env.get_string(&s.into()).ok().map(String::from)).unwrap_or_default();
                            why = text.lines().next().unwrap_or("").to_owned();
                            match env.call_method(&e, "getCause", "()Ljava/lang/Throwable;", &[]).ok().and_then(|c| c.l().ok()).filter(|c| !c.is_null()) {
                                Some(cause) => e = cause.into(),
                                None => break,
                            }
                        }
                    }
                    failed.push(format!("{target}: {why}"));
                }
            }
        }
        eprintln!("MIXIN REPORT: {ok} target classes loaded, {} failed", failed.len());
        for f in &failed { eprintln!("  FAILED {f}"); }
    }).unwrap();
}
