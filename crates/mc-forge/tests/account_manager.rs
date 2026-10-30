//! Boots the real AccountManager 1.8 jar on the Rust-backed API and drives its server-list button and screen.
//! Needs: stubs jar (tools/shadow/stubs.sh), a JDK 8, the mod jar and Minecraft's libraries in RUSTMC_FORGE_CP.
//! `tools/shadow/test-mods.sh` sets all of that up and runs this test.
use jni::objects::{JObject, JValue};
use mc_forge::{game, impls::{self, Draw}, runtime, stubs};

fn list_len(env: &mut jni::JNIEnv<'_>, list: &JObject<'_>) -> i32 { env.call_method(list, "size", "()I", &[]).unwrap().i().unwrap() }

/// Clicks the button labelled `label` on the current Java screen, at its centre.
fn click_button(env: &mut jni::JNIEnv<'_>, label: &str) {
    let screen = game::current_screen(env).unwrap().unwrap();
    let list = env.get_field(&screen, "field_146292_n", "Ljava/util/List;").unwrap().l().unwrap();
    for i in 0..list_len(env, &list) {
        let b = env.call_method(&list, "get", "(I)Ljava/lang/Object;", &[JValue::Int(i)]).unwrap().l().unwrap();
        let t = env.get_field(&b, "field_146126_j", "Ljava/lang/String;").unwrap().l().unwrap();
        if String::from(env.get_string(&t.into()).unwrap()) != label { continue; }
        let int = |env: &mut jni::JNIEnv<'_>, f: &str| env.get_field(&b, f, "I").unwrap().i().unwrap();
        let (x, y) = (int(env, "field_146128_h") + int(env, "field_146120_f") / 2, int(env, "field_146129_i") + int(env, "field_146121_g") / 2);
        // The full LWJGL path: press then release go through the screen's handleMouseInput.
        game::mouse_event(env, x, y, 0, true, 0).unwrap();
        return game::mouse_event(env, x, y, 0, false, 0).unwrap();
    }
    let class = env.get_object_class(&screen).unwrap();
    let name = env.call_method(&class, "getName", "()Ljava/lang/String;", &[]).unwrap().l().unwrap();
    panic!("no button {label} on {}", String::from(env.get_string(&name.into()).unwrap()));
}

#[test]
fn account_manager_opens_its_screen_from_the_server_list() {
    // The JVM runs Java on the calling thread: LaunchClassLoader plus class loading needs more than a test thread's 2 MiB.
    std::thread::Builder::new().stack_size(64 << 20).spawn(account_manager_flow).unwrap().join().unwrap();
}

fn account_manager_flow() {
    let (Some(jar), Some(libs), Some(jvm), Some(mod_jar)) = (std::env::var_os("RUSTMC_STUBS_JAR"), std::env::var_os("RUSTMC_FORGE_CP"),
        runtime::find_libjvm(), std::env::var_os("RUSTMC_MOD_JAR")) else { eprintln!("skipped: run tools/shadow/test-mods.sh"); return };
    // The mod writes accounts.json into the working directory.
    let work = std::env::temp_dir().join("rustmc-forge-test");
    std::fs::create_dir_all(&work).unwrap();
    std::env::set_current_dir(&work).unwrap();
    let mod_jar = std::fs::canonicalize(mod_jar).unwrap();
    let mut cp = vec![std::fs::canonicalize(jar).unwrap(), mod_jar.clone()];
    cp.extend(std::env::split_paths(&libs));
    runtime::start(&jvm, &cp, &[mod_jar.clone()]).unwrap();
    assert_eq!(runtime::load_mods(&[mod_jar]).unwrap(), ["accountmanager"]);

    runtime::with_env(|env| {
        // A window that is not exactly 2x the GUI size, as on a real display.
        impls::set_display((640, 338), (1918, 1016));
        // AccountManager adds its "Accounts" button to GuiMultiplayer and GuiSelectWorld.
        let menu = runtime::new_object(env, "net/minecraft/client/gui/GuiMultiplayer", "(Lnet/minecraft/client/gui/GuiScreen;)V", &[JValue::Object(&JObject::null())]).unwrap();
        game::open_screen(env, &menu).unwrap();
        game::client_tick(env, true).unwrap();
        game::client_tick(env, false).unwrap();
        let buttons = env.get_field(&menu, "field_146292_n", "Ljava/util/List;").unwrap().l().unwrap();
        let n = list_len(env, &buttons);
        eprintln!("server list buttons added by the mod: {n}");
        assert!(n >= 1, "AccountManager adds a button to the server list");

        let button = env.call_method(&buttons, "get", "(I)Ljava/lang/Object;", &[JValue::Int(0)]).unwrap().l().unwrap();
        let int = |env: &mut jni::JNIEnv<'_>, f: &str| env.get_field(&button, f, "I").unwrap().i().unwrap();
        let (x, y) = (int(env, "field_146128_h") + 1, int(env, "field_146129_i") + 1);
        game::draw_screen(env, x, y, 0.0).unwrap();
        impls::take_draw();
        game::mouse_click(env, x, y, 0).unwrap();

        let screen = game::current_screen(env).unwrap().expect("clicking opened the mod's screen");
        let class = env.get_object_class(&screen).unwrap();
        let name = env.call_method(&class, "getName", "()Ljava/lang/String;", &[]).unwrap().l().unwrap();
        let name: String = env.get_string(&name.into()).unwrap().into();
        eprintln!("opened screen: {name}");
        assert!(name.starts_with("me.ksyz.accountmanager."));
        game::draw_screen(env, 10, 10, 0.0).unwrap();
        let drawn = impls::take_draw();
        let texts: Vec<_> = drawn.iter().filter_map(|d| if let Draw::Text { text, .. } = d { Some(text.as_str()) } else { None }).collect();
        eprintln!("drew {} commands, text: {texts:?}", drawn.len());
        for want in ["Login", "Add", "Delete", "Cancel"] {
            assert!(texts.contains(&want), "button {want} drawn: {texts:?}");
        }
        assert!(drawn.iter().any(|d| matches!(d, Draw::Vertices { .. })), "button backgrounds reach the tessellator");

        // Typing: Add -> Cracked opens a username field; key events must reach it through the real GuiTextField.
        let texts_after = |env: &mut jni::JNIEnv<'_>| -> Vec<String> {
            game::draw_screen(env, 0, 0, 0.0).unwrap();
            impls::take_draw().into_iter().filter_map(|d| if let Draw::Text { text, .. } = d { Some(text) } else { None }).collect()
        };
        click_button(env, "Add");
        click_button(env, "Cracked");
        for ch in "hello".chars() { game::key_typed(env, ch, 0).unwrap(); }
        let texts = texts_after(env);
        eprintln!("cracked screen text: {texts:?}");
        assert!(texts.iter().any(|t| t.contains("hello")), "typed text reaches the field: {texts:?}");
        // ctrl+V pastes the system clipboard (here a fixed string) through Rust, not AWT.
        impls::set_clipboard(|| "PASTED".into(), |_| {});
        impls::set_modifiers(true, false, false);
        game::key_typed(env, '\u{16}', 47).unwrap();
        impls::set_modifiers(false, false, false);
        assert!(texts_after(env).iter().any(|t| t.contains("PASTED")), "paste reaches the field");
        game::key_typed(env, '\u{8}', 14).unwrap(); // backspace
        assert!(texts_after(env).iter().any(|t| t.contains("helloPASTE") && !t.contains("PASTED")), "backspace edits it");
        // The mod copies and opens links through AWT; the transformer sends both to Rust.
        let copied = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let sink = copied.clone();
        impls::set_clipboard(|| String::new(), move |t| *sink.lock().unwrap() = t.to_owned());
        let utils = runtime::class(env, "me/ksyz/accountmanager/utils/SystemUtils").unwrap();
        let text = env.new_string("https://login.example/auth").unwrap();
        env.call_static_method(&utils, "setClipboard", "(Ljava/lang/String;)V", &[JValue::Object(&text)]).unwrap();
        assert_eq!(*copied.lock().unwrap(), "https://login.example/auth", "setClipboard reaches Rust, not AWT");
        // An account switcher replaces Minecraft.session by reflection; the client reads it back.
        assert_eq!(game::session(env).unwrap(), ("Player".into(), String::new(), "0".into()));
        let mc = impls::minecraft(env).unwrap();
        let strs: Vec<_> = ["Bob", "uuid-1", "tok", "mojang"].iter().map(|v| env.new_string(v).unwrap()).collect();
        let args: Vec<_> = strs.iter().map(|s| JValue::Object(&**s)).collect();
        let session = runtime::new_object(env, "net/minecraft/util/Session", "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V", &args).unwrap();
        env.set_field(&mc, "field_71449_j", "Lnet/minecraft/util/Session;", JValue::Object(&session)).unwrap();
        assert_eq!(game::session(env).unwrap(), ("Bob".into(), "uuid-1".into(), "tok".into()));
        game::key_typed(env, '\0', 1).unwrap(); // escape returns to the parent screen
        assert!(texts_after(env).iter().any(|t| t == "Cancel" || t == "Microsoft"), "escape goes back");
    }).unwrap();
    eprintln!("still stubbed: {:#?}", stubs::stubbed());
}
