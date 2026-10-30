#[path = "../src/launch.rs"]
mod launch;
use launch::parse;

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}
const CP: &str = "/p/NewLaunch.jar:/p/libraries/com/mojang/netty/1.8.8/netty-1.8.8.jar:/p/libraries/com/mojang/minecraft/1.8.9/minecraft-1.8.9-client.jar";
// Script layout per MinecraftInstance::createLaunchScript (MinecraftInstance.cpp#L812-L905), params per
// the 1.8.9 meta minecraftArguments.
const SCRIPT: &str = "mainClass net.minecraft.client.main.Main\nparam --username\nparam Steve\nparam --version\nparam 1.8.9\nparam --gameDir\nparam /i/minecraft\nparam --assetsDir\nparam /p/assets\nparam --assetIndex\nparam 1.8\nparam --uuid\nparam abc123\nparam --accessToken\nparam tok.en\nparam --userProperties\nparam {}\nparam --userType\nparam msa\nwindowTitle Minecraft: 1.8.9\nwindowParams 1280x720\nlauncherBrand PrismLauncher\nuserName Steve\nsessionId token:tok.en:abc123\ntraits FirstThreadOnMacOS\nlauncher standard\nlaunch\n";

fn noenv(_: &str) -> Option<String> {
    None
}

#[test]
fn wrapper_form_with_stdin_script() {
    let argv = s(&["/j/java", "-Xmx2G", "-Djava.library.path=/i/natives", "-cp", CP, "org.prismlauncher.EntryPoint"]);
    let li = parse(&argv, Some(SCRIPT), &noenv).unwrap();
    assert_eq!(li.username, "Steve");
    assert_eq!(li.uuid, "abc123");
    assert_eq!(li.access_token, "tok.en");
    assert_eq!(li.user_type, "msa");
    assert_eq!(li.game_dir, "/i/minecraft");
    assert_eq!(li.assets_dir, "/p/assets");
    assert_eq!(li.asset_index, "1.8");
    assert_eq!(li.client_jar.as_deref(), Some("/p/libraries/com/mojang/minecraft/1.8.9/minecraft-1.8.9-client.jar"));
    assert_eq!((li.width, li.height), (Some(1280), Some(720)));
    assert_eq!(li.server, None);
}

#[test]
fn quick_join_and_maximized() {
    let script = SCRIPT
        .replace("windowParams 1280x720", "windowParams maximized")
        .replace("launcher standard", "serverAddress mc.example.com\nserverPort 25566");
    let li = parse(&[], Some(&script), &noenv).unwrap();
    assert_eq!((li.width, li.height), (None, None));
    assert_eq!(li.server.as_deref(), Some("mc.example.com"));
    assert_eq!(li.port, Some(25566));
}

#[test]
fn direct_form_argv() {
    let argv = s(&[
        "java", "-cp", CP, "net.minecraft.client.main.Main", "--username", "Alex", "--gameDir", "/g", "--assetsDir", "/a",
        "--assetIndex", "1.8", "--uuid", "u", "--accessToken", "t", "--userType", "legacy", "--width", "800", "--height",
        "600", "--server", "h", "--port", "1",
    ]);
    let li = parse(&argv, None, &noenv).unwrap();
    assert_eq!(li.username, "Alex");
    assert_eq!(li.user_type, "legacy");
    assert_eq!((li.width, li.height, li.port), (Some(800), Some(600), Some(1)));
}

#[test]
fn errors_and_env_fallback() {
    assert!(parse(&[], Some("abort\n"), &noenv).is_err());
    assert!(parse(&[], Some("param --username\nparam X\nlaunch\n"), &noenv).is_err());
    let no_gd = SCRIPT.replace("param --gameDir\nparam /i/minecraft\n", "");
    let env = |k: &str| (k == "INST_MC_DIR").then(|| "/env/mc".to_string());
    assert_eq!(parse(&[], Some(&no_gd), &env).unwrap().game_dir, "/env/mc");
}

// Captured from a real Prism 11.1.0 launch (macOS, quick-join); token, name and uuid replaced.
// Differences from the reconstruction: serverAddress/serverPort come before the params, plus
// launcherVersion/instanceName/instanceIcon* lines, windowParams 854x480.
#[test]
fn real_prism_script() {
    let script = "mainClass net.minecraft.client.main.Main\nserverAddress 127.0.0.1\nserverPort 25599\nparam --username\nparam Steve\nparam --version\nparam 1.8.9\nparam --gameDir\nparam /i/rustmc-dev/minecraft\nparam --assetsDir\nparam /p/assets\nparam --assetIndex\nparam 1.8\nparam --uuid\nparam 00000000000040008000000000000000\nparam --accessToken\nparam REDACTED\nparam --userProperties\nparam {}\nparam --userType\nparam msa\nwindowTitle Prism Launcher: rustmc-dev\nwindowParams 854x480\nlauncherBrand PrismLauncher\nlauncherVersion 11.1.0\ninstanceName rustmc-dev\ninstanceIconKey rustmc-dev\ninstanceIconPath icon.png\nuserName Steve\nsessionId token:REDACTED:00000000000040008000000000000000\nlauncher standard\nlaunch\n";
    let argv = s(&["/j/java", "-cp", "/Applications/Prism Launcher.app/Contents/MacOS/jars/NewLaunch.jar:/p/libraries/com/mojang/minecraft/1.8.9/minecraft-1.8.9-client.jar", "org.prismlauncher.EntryPoint"]);
    let li = parse(&argv, Some(script), &noenv).unwrap();
    assert_eq!((li.width, li.height), (Some(854), Some(480)));
    assert_eq!((li.server.as_deref(), li.port), (Some("127.0.0.1"), Some(25599)));
    assert_eq!(li.access_token, "REDACTED");
    assert!(li.client_jar.unwrap().ends_with("minecraft-1.8.9-client.jar"));
}
