//! Drives one real mod (RUSTMC_CORPUS_JAR) through the live game loop headlessly: load, a world with players, client
//! and player ticks, HUD overlay, world-last rendering, chat, keys and the main menu. It never fails on a mod error;
//! it writes every unimplemented stub the mod reached to `tests/forge/build/<jar>-stubbed.txt`: the to-do list.
//! Optional: RUSTMC_STUBS_JAR (default: the runtime manifest's stub jar, or a mixin-scan build of it).
use jni::{objects::JValue, JNIEnv};
use mc_forge::{game, hooks, impls, live, runtime, stubs};
use serde_json::{json, Value};
use std::path::PathBuf;

fn entity(id: i32, class: &str, name: &str, x: f64) -> Value {
    json!({"id":id,"class":class,"name":name,"uuid":format!("00000000-0000-0000-0000-{id:012}"),
        "x":x,"y":64.,"z":-2.,"prev_x":x,"prev_y":64.,"prev_z":-2.,"last_x":x,"last_y":64.,"last_z":-2.,
        "motion_x":0.,"motion_y":0.,"motion_z":0.,"yaw":0.,"pitch":0.,"prev_yaw":0.,"prev_pitch":0.,
        "flags":0,"on_ground":true,"dead":false,"no_clip":false,"ticks":5,"width":0.6,"height":1.8,"fall_distance":0.,"health":20.,"hurt_time":0})
}

fn snapshot(tick: i64) -> Value {
    let mut inventory = vec![Value::Null; 40];
    inventory[0] = json!({"id":276,"class":"net.minecraft.item.ItemSword","name":"minecraft:diamond_sword","max":1,"count":1,"damage":0,"nbt":null});
    json!({"generation":1,"active":true,"dimension":0,"time":6000+tick,"total_time":18000+tick,"partial_ticks":0.5,
        "entities":[entity(1,"net.minecraft.client.entity.EntityPlayerSP","Local",0.),
                    entity(2,"net.minecraft.client.entity.EntityOtherPlayerMP","Remote",3.)],
        "selected_slot":0,"inventory":inventory,"input":{"strafe":0.,"forward":0.,"jump":false,"sneak":false},
        "capabilities":{"flying":false,"allow_flying":false,"creative":false,"invulnerable":false},
        "settings":{"third_person":0,"fov":70.,"sensitivity":0.5,"render_distance":8,
            "keys":{"key.attack":-100,"key.use":-99,"key.forward":17,"key.left":30,"key.back":31,"key.right":32,"key.jump":57,
                "key.sneak":42,"key.sprint":29,"key.inventory":18,"key.drop":16,"key.chat":20,"key.playerlist":15,"key.hotbar.1":2}},
        "tab":{"players":[{"uuid":"00000000-0000-0000-0000-000000000001","name":"Local","display":null,"ping":20,"game_mode":0},
                          {"uuid":"00000000-0000-0000-0000-000000000002","name":"Remote","display":"§cRemote","ping":80,"game_mode":0}],
               "teams":[{"name":"red","display":"Red","prefix":"§c","suffix":"","color":12,"friendly_flags":3,"visibility":"always","members":["Remote"]}],
               "header":null,"footer":null},
        "view":{"x":0.,"y":64.,"z":-2.,"yaw":0.,"pitch":0.,"target":{"type":"entity","id":2,"hx":2.7,"hy":65.,"hz":-2.}}})
}

/// Runs one step; a Java failure is printed with its stack and the run continues.
fn step(env: &mut JNIEnv<'_>, label: &str, f: impl FnOnce(&mut JNIEnv<'_>) -> jni::errors::Result<()>) {
    if let Err(e) = f(env) {
        eprintln!("== {label}: {e}");
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
}

#[test]
fn drive_corpus_mod_through_the_live_loop() {
    std::thread::Builder::new().stack_size(128 << 20).spawn(flow).unwrap().join().unwrap();
}

fn flow() {
    let Some(jar) = std::env::var_os("RUSTMC_CORPUS_JAR") else { eprintln!("skipped: set RUSTMC_CORPUS_JAR"); return };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let build = root.join("tests/forge/build");
    let config: Value = serde_json::from_slice(&std::fs::read(build.join("runtime.json")).unwrap()).unwrap();
    let jar = std::fs::canonicalize(jar).unwrap();
    let stubs_jar = std::env::var_os("RUSTMC_STUBS_JAR").map(|p| std::fs::canonicalize(p).unwrap()).unwrap_or_else(|| config["stubsJar"].as_str().unwrap().into());
    let mut cp = vec![stubs_jar, jar.clone()];
    // Corpus mods shade their own Mixin: Minecraft's libraries only, with the Guava Mixin 0.8 needs.
    let guava21 = PathBuf::from(std::env::var("APPDATA").unwrap()).join("PrismLauncher/libraries/com/google/guava/guava/21.0/guava-21.0.jar");
    for lib in config["libraries"].as_array().unwrap() {
        let lib = PathBuf::from(lib.as_str().unwrap());
        cp.push(if lib.file_name().is_some_and(|n| n == "guava-17.0.jar") && guava21.is_file() { guava21.clone() } else { lib });
    }
    std::env::set_current_dir(&build).unwrap();
    // Some hosts deny this binary writes to %TEMP%; mods' temp files (AWT fonts, natives) go under the build dir.
    let tmp = build.join("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    if std::env::var_os("RUSTMC_JVM_OPTS").is_none() { std::env::set_var("RUSTMC_JVM_OPTS", format!("-Djava.io.tmpdir={}", tmp.display())); }
    runtime::start(&PathBuf::from(config["libjvm"].as_str().unwrap()), &cp, std::slice::from_ref(&jar)).unwrap();
    impls::set_display((427, 240), (854, 480));
    impls::set_session("Local", "00000000000000000000000000000001");
    match runtime::load_mods(std::slice::from_ref(&jar)) {
        Ok(ids) => eprintln!("== loaded mods: {ids:?}"),
        Err(e) => eprintln!("== load failed: {e}"),
    }
    let hook = |label: &str, r: Option<jni::errors::Result<bool>>| {
        if let Some(Err(e)) = r {
            eprintln!("== {label}: {e}");
            runtime::with_env(|env| { let _ = env.exception_describe(); let _ = env.exception_clear(); });
        }
    };
    hook("startGame", hooks::start_game());
    runtime::with_env(|env| {
        step(env, "main menu", |env| { game::open_shadow(env, "main_menu")?; game::draw_screen(env, 10, 10, 0.) });
        step(env, "close menu", game::close_shadow);
        step(env, "publish", |env| live::publish(env, &snapshot(0)));
        step(env, "loadWorld", |env| hooks::load_world(env).map(drop));
    });
    // A perspective camera 2 blocks behind the origin, for world-last draws.
    let projection = [1., 0., 0., 0., 0., 1.78, 0., 0., 0., 0., -1., -1., 0., 0., -0.1, 0.];
    let modelview = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., -1.62, -2., 1.];
    for tick in 0..40 {
        hook("runTick", hooks::run_tick(&mut || { runtime::with_env(|env| {
            step(env, "publish", |env| live::publish(env, &snapshot(tick)));
            step(env, "client tick start", |env| game::client_tick(env, true));
            step(env, "player tick", |env| { game::player_tick(env, true)?; game::living_update(env)?; game::player_tick(env, false) });
            step(env, "client tick end", |env| game::client_tick(env, false));
        }); }));
        hook("runGameLoop", hooks::game_loop(&mut || { runtime::with_env(|env| {
            step(env, "render tick", |env| game::render_tick(env, true, 0.5));
            step(env, "world last", |env| game::world_last(env, 0.5, projection, modelview, (427., 240.)));
            step(env, "overlay", |env| {
                game::overlay(env, 0.5, true, "ALL")?;
                game::overlay_text(env, 0.5, &[], &[])?;
                game::overlay(env, 0.5, false, "ALL").map(drop)
            });
            step(env, "render tick end", |env| game::render_tick(env, false, 0.5));
            step(env, "changes", |env| live::changes(env).map(drop));
        }); }));
        impls::take_draw();
    }
    hook("clickMouse", hooks::click(hooks::Click::Left, &mut || {}));
    hook("rightClickMouse", hooks::click(hooks::Click::Right, &mut || {}));
    runtime::with_env(|env| {
        step(env, "chat", |env| game::chat_received(env, 0, r#"{"text":"Hello from the server"}"#).map(drop));
        // Right shift: a common click-GUI key.
        impls::set_key(54, true, None, false);
        step(env, "key", |env| { impls::key_bindings(env, 54, true)?; game::key_input(env) });
        impls::set_key(54, false, None, false);
        step(env, "key up", |env| { impls::key_bindings(env, 54, false)?; game::key_input(env) });
        step(env, "attack", |env| game::attack_entity(env, 2).map(drop));
        step(env, "screen", |env| { if game::current_screen(env)?.is_some() { game::draw_screen(env, 100, 100, 0.5)?; } Ok(()) });
        let _ = env.call_static_method("java/lang/System", "gc", "()V", &[]);
        let _: Option<JValue> = None;
    });
    let stubbed = stubs::stubbed();
    let name = jar.file_stem().unwrap().to_string_lossy().to_string();
    let out = build.join(format!("{name}-stubbed.txt"));
    std::fs::write(&out, stubbed.join("\n")).unwrap();
    eprintln!("== {} unimplemented stubs reached -> {}", stubbed.len(), out.display());
}
