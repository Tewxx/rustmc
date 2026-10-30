//! Embedded JVM that hosts the stubbed Minecraft/Forge API and Forge mods. One per process.
use crate::{impls, stubs};
use jni::{errors::Result as JniResult, objects::{GlobalRef, JClass, JObject, JValue}, InitArgsBuilder, JNIEnv, JNIVersion, JavaVM};
use std::{collections::HashMap, path::{Path, PathBuf}, sync::{Mutex, OnceLock}};

static VM: OnceLock<JavaVM> = OnceLock::new();
/// `Launch.classLoader`: stubs, mods and mixed-in classes all live here, never in the system loader.
static LOADER: OnceLock<GlobalRef> = OnceLock::new();
static CLASSES: Mutex<Option<HashMap<String, GlobalRef>>> = Mutex::new(None);

/// A class as the LaunchClassLoader sees it (`net/minecraft/client/Minecraft`). `FindClass` from a thread with no
/// Java frame would use the system loader and find a different copy, so Rust always goes through here.
pub fn class<'l>(env: &mut JNIEnv<'l>, name: &str) -> JniResult<JClass<'l>> {
    if let Some(g) = CLASSES.lock().unwrap().get_or_insert_with(Default::default).get(name).cloned() {
        return Ok(JClass::from(env.new_local_ref(g.as_obj())?));
    }
    let loader = LOADER.get().ok_or(jni::errors::Error::JavaException)?.clone();
    let dotted = env.new_string(name.replace('/', "."))?;
    let c = env.call_method(loader.as_obj(), "loadClass", "(Ljava/lang/String;)Ljava/lang/Class;", &[JValue::Object(&dotted)])?.l()?;
    let global = env.new_global_ref(&c)?;
    CLASSES.lock().unwrap().get_or_insert_with(Default::default).insert(name.to_owned(), global);
    Ok(JClass::from(c))
}

pub fn new_object<'l>(env: &mut JNIEnv<'l>, class_name: &str, sig: &str, args: &[JValue]) -> JniResult<JObject<'l>> {
    let c = class(env, class_name)?;
    env.new_object(c, sig, args)
}

pub fn alloc<'l>(env: &mut JNIEnv<'l>, class_name: &str) -> JniResult<JObject<'l>> {
    let c = class(env, class_name)?;
    env.alloc_object(c)
}

pub fn call_static<'l>(env: &mut JNIEnv<'l>, class_name: &str, method: &str, sig: &str, args: &[JValue]) -> JniResult<jni::objects::JValueOwned<'l>> {
    let c = class(env, class_name)?;
    env.call_static_method(c, method, sig, args)
}

pub fn static_field<'l>(env: &mut JNIEnv<'l>, class_name: &str, field: &str, sig: &str) -> JniResult<jni::objects::JValueOwned<'l>> {
    let c = class(env, class_name)?;
    env.get_static_field(c, field, sig)
}

/// `libjvm` is the JDK 8 `libjvm.{so,dylib,dll}`; `classpath` must hold the stub jar (and any mod jars).
/// Called from `BootTweaker` inside `Launch.main`: records the LaunchClassLoader, binds both copies of `Stub` and
/// registers the Rust handlers.
extern "system" fn on_loader(mut env: JNIEnv<'_>, _class: JClass<'_>) {
    let result = (|| -> JniResult<()> {
        let launch = env.find_class("net/minecraft/launchwrapper/Launch")?;
        let loader = env.get_static_field(&launch, "classLoader", "Lnet/minecraft/launchwrapper/LaunchClassLoader;")?.l()?;
        let _ = LOADER.set(env.new_global_ref(loader)?);
        let stub = class(&mut env, "rustmc/forge/Stub")?;
        stubs::bind(&mut env, &stub)?;
        // LaunchWrapper keeps org.lwjgl.* in the system loader, so those stubs call that loader's own copy of Stub.
        let system_stub = env.find_class("rustmc/forge/Stub")?;
        stubs::bind(&mut env, &system_stub)?;
        impls::register_all();
        Ok(())
    })();
    if let Err(e) = result {
        let _ = env.exception_describe();
        eprintln!("forge: binding stubs failed: {e}");
    }
}

/// `mod_jars` are on `classpath` too; their manifest `TweakClass` entries (Mixin) are launched under LaunchWrapper.
pub fn start(libjvm: &Path, classpath: &[PathBuf], mod_jars: &[PathBuf]) -> Result<(), String> {
    if VM.get().is_some() { return Ok(()); }
    let cp = std::env::join_paths(classpath).map_err(|e| e.to_string())?;
    let option = format!("-Djava.class.path={}", cp.to_string_lossy());
    // Headless: on macOS AWT replaces the NSApplication delegate winit owns, which panics the window loop (mods touch the
    // clipboard, Desktop or file choosers). Mods still run; AWT windows just cannot open.
    let mut builder = InitArgsBuilder::new().version(JNIVersion::V8).option(&option).option("-Djava.awt.headless=true");
    // e.g. RUSTMC_JVM_OPTS="-Xcheck:jni -Xmx2g"
    let extra_opts = std::env::var("RUSTMC_JVM_OPTS").unwrap_or_default();
    for extra in extra_opts.split_whitespace() { builder = builder.option(extra); }
    let args = builder.build().map_err(|e| e.to_string())?;
    let vm = JavaVM::with_libjvm(args, || Ok(libjvm)).map_err(|e| e.to_string())?;
    let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
    let err = |e: jni::errors::Error, env: &mut JNIEnv<'_>| { let _ = env.exception_describe(); e.to_string() };
    let array = env.new_object_array(mod_jars.len() as i32, "java/lang/String", JObject::null()).map_err(|e| err(e, &mut env))?;
    for (i, jar) in mod_jars.iter().enumerate() {
        let s = env.new_string(jar.to_string_lossy()).map_err(|e| err(e, &mut env))?;
        env.set_object_array_element(&array, i as i32, s).map_err(|e| err(e, &mut env))?;
    }
    // LaunchWrapper boots under the system loader; BootTweaker calls back into `on_loader` as soon as the
    // LaunchClassLoader exists, so stubs are bound before Mixin and the mods run. Afterwards the rest of the game is
    // reached through Launch.classLoader.
    let launcher = env.find_class("rustmc/boot/Launcher").map_err(|e| format!("stub jar not on class path: {}", err(e, &mut env)))?;
    env.register_native_methods(&launcher, &[jni::NativeMethod { name: "onLoader".into(), sig: "()V".into(), fn_ptr: on_loader as *mut std::ffi::c_void }])
        .map_err(|e| err(e, &mut env))?;
    env.call_static_method(&launcher, "start", "([Ljava/lang/String;)V", &[JValue::Object(&array)]).map_err(|e| err(e, &mut env))?;
    if LOADER.get().is_none() { return Err("LaunchWrapper never reached rustmc.boot.BootTweaker".into()); }
    drop(env);
    let _ = VM.set(vm);
    Ok(())
}

/// Runs the `@Mod` lifecycle (pre-init, init, post-init) for every mod in `jars`; they must already be on the
/// class path. Returns the ids of the mods that were constructed.
pub fn load_mods(jars: &[PathBuf]) -> Result<Vec<String>, String> {
    with_env(|env| -> jni::errors::Result<Vec<String>> {
        let array = env.new_object_array(jars.len() as i32, "java/lang/String", JObject::null())?;
        for (i, jar) in jars.iter().enumerate() {
            let s = env.new_string(jar.to_string_lossy())?;
            env.set_object_array_element(&array, i as i32, s)?;
        }
        let ids = call_static(env, "rustmc/forge/ModLoader", "load", "([Ljava/lang/String;)[Ljava/lang/String;", &[JValue::Object(&array)])?.l()?;
        let ids = jni::objects::JObjectArray::from(ids);
        (0..env.get_array_length(&ids)?).map(|i| {
            let s = env.get_object_array_element(&ids, i)?;
            Ok(env.get_string(&jni::objects::JString::from(s))?.into())
        }).collect()
    }).ok_or("forge runtime not started")?.map_err(|e| e.to_string())
}

/// Runs `f` on the calling thread attached to the embedded JVM; `None` if `start` has not succeeded.
pub fn with_env<R>(f: impl FnOnce(&mut jni::JNIEnv<'_>) -> R) -> Option<R> {
    let mut env = VM.get()?.attach_current_thread().ok()?;
    Some(f(&mut env))
}

/// libjvm inside `$RUSTMC_JVM` (a libjvm file) or under `$JAVA_HOME` (a JDK 8 home).
pub fn find_libjvm() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("RUSTMC_JVM") { return Some(PathBuf::from(p)); }
    let home = PathBuf::from(std::env::var_os("JAVA_HOME")?);
    ["jre/lib/server/libjvm.dylib", "jre/lib/amd64/server/libjvm.so", "jre/lib/aarch64/server/libjvm.so", "jre/bin/server/jvm.dll"]
        .iter().map(|p| home.join(p)).find(|p| p.is_file())
}
