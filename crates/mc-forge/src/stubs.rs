//! Rust side of the generated API stubs (`tools/shadow/stubs.sh`). Every stubbed Java method calls
//! `rustmc.forge.Stub.call(key, self, args)`. To implement a method, `register` its key:
//!
//! ```ignore
//! register("net/minecraft/util/MathHelper.func_76128_c", |c| Ok(math::floor_double(c.f64(0)?).into()));
//! ```
//!
//! A key is `owner.name(desc)`; `owner.name` alone matches every overload and `owner.*` every method of a class. Anything unregistered is logged
//! once as `[forge] stubbed: <key>` and returns null (Java turns that into 0/false/null).
use jni::{errors::Result, objects::{GlobalRef, JClass, JObject, JObjectArray, JString}, JNIEnv, NativeMethod};
use std::{collections::{BTreeSet, HashMap}, ffi::c_void, sync::{Arc, Mutex}};

/// One stubbed call: the receiver (null for statics) and typed access to its boxed arguments.
pub struct Call<'a, 'l> {
    pub env: &'a mut JNIEnv<'l>,
    pub this: JObject<'l>,
    args: JObjectArray<'l>,
}

impl<'l> Call<'_, 'l> {
    pub fn arguments(&self) -> &JObjectArray<'l> { &self.args }
    pub fn obj(&mut self, i: usize) -> Result<JObject<'l>> { self.env.get_object_array_element(&self.args, i as i32) }
    pub fn i32(&mut self, i: usize) -> Result<i32> { let o = self.obj(i)?; self.env.call_method(&o, "intValue", "()I", &[])?.i() }
    pub fn i64(&mut self, i: usize) -> Result<i64> { let o = self.obj(i)?; self.env.call_method(&o, "longValue", "()J", &[])?.j() }
    pub fn f32(&mut self, i: usize) -> Result<f32> { let o = self.obj(i)?; self.env.call_method(&o, "floatValue", "()F", &[])?.f() }
    pub fn f64(&mut self, i: usize) -> Result<f64> { let o = self.obj(i)?; self.env.call_method(&o, "doubleValue", "()D", &[])?.d() }
    pub fn char(&mut self, i: usize) -> Result<char> {
        let o = self.obj(i)?;
        let unit = self.env.call_method(&o, "charValue", "()C", &[])?.c()?;
        Ok(char::from_u32(unit as u32).unwrap_or('\u{fffd}'))
    }
    pub fn bool(&mut self, i: usize) -> Result<bool> { let o = self.obj(i)?; self.env.call_method(&o, "booleanValue", "()Z", &[])?.z() }
    pub fn string(&mut self, i: usize) -> Result<Option<String>> {
        let o = self.obj(i)?;
        if o.is_null() { return Ok(None); }
        Ok(Some(self.env.get_string(&JString::from(o))?.into()))
    }
}

/// What a handler hands back to Java; objects are kept alive by the `GlobalRef`.
pub enum Ret { Null, Bool(bool), Int(i32), Long(i64), Float(f32), Double(f64), Str(String), Obj(GlobalRef) }
impl From<()> for Ret { fn from(_: ()) -> Self { Ret::Null } }
impl From<bool> for Ret { fn from(v: bool) -> Self { Ret::Bool(v) } }
impl From<i32> for Ret { fn from(v: i32) -> Self { Ret::Int(v) } }
impl From<i64> for Ret { fn from(v: i64) -> Self { Ret::Long(v) } }
impl From<f32> for Ret { fn from(v: f32) -> Self { Ret::Float(v) } }
impl From<f64> for Ret { fn from(v: f64) -> Self { Ret::Double(v) } }
impl From<&str> for Ret { fn from(v: &str) -> Self { Ret::Str(v.into()) } }
impl From<String> for Ret { fn from(v: String) -> Self { Ret::Str(v) } }
impl From<GlobalRef> for Ret { fn from(v: GlobalRef) -> Self { Ret::Obj(v) } }

type Handler = dyn for<'a, 'l> Fn(&mut Call<'a, 'l>) -> Result<Ret> + Send + Sync;

#[derive(Default)]
struct Registry {
    handlers: HashMap<String, Arc<Handler>>,
    stubbed: BTreeSet<String>,
}

static REGISTRY: Mutex<Option<Registry>> = Mutex::new(None);

fn registry<R>(f: impl FnOnce(&mut Registry) -> R) -> R { f(REGISTRY.lock().unwrap().get_or_insert_with(Default::default)) }

pub fn register<F>(key: &str, handler: F)
where F: for<'a, 'l> Fn(&mut Call<'a, 'l>) -> Result<Ret> + Send + Sync + 'static {
    registry(|r| r.handlers.insert(key.to_owned(), Arc::new(handler)));
}

/// Every registered handler key (`class.method`, `class.*`, `*.method`), sorted: what Rust implements.
pub fn registered() -> Vec<String> { let mut k: Vec<String> = registry(|r| r.handlers.keys().cloned().collect()); k.sort(); k }

/// Every unimplemented method hit so far, sorted: the to-do list for what real mods need.
pub fn stubbed() -> Vec<String> { registry(|r| r.stubbed.iter().cloned().collect()) }

/// Exact key first, then `owner.name`; an unregistered key is logged the first time it is hit.
fn resolve(key: &str) -> Option<Arc<Handler>> {
    registry(|r| {
        let short = key.split('(').next().unwrap_or(key);
        let class_wide = short.split('.').next().map(|class| format!("{class}.*"));
        let inherited = short.rsplit_once('.').map(|(_, method)| format!("*.{method}"));
        let found = r.handlers.get(key).or_else(|| r.handlers.get(short))
            .or_else(|| class_wide.as_ref().and_then(|k| r.handlers.get(k)))
            .or_else(|| inherited.as_ref().filter(|_| short.starts_with("net/minecraft/")).and_then(|k| r.handlers.get(k))).cloned();
        // Static initializers of every stubbed class run once; they are expected, not a to-do.
        if found.is_none() && !short.ends_with(".<clinit>") && r.stubbed.insert(key.to_owned()) {
            eprintln!("[forge] stubbed: {key}");
        }
        found
    })
}

fn to_java<'l>(env: &mut JNIEnv<'l>, ret: Ret) -> Result<JObject<'l>> {
    use jni::objects::JValue as V;
    Ok(match ret {
        Ret::Null => JObject::null(),
        Ret::Bool(v) => env.new_object("java/lang/Boolean", "(Z)V", &[V::Bool(v.into())])?,
        Ret::Int(v) => env.new_object("java/lang/Integer", "(I)V", &[V::Int(v)])?,
        Ret::Long(v) => env.new_object("java/lang/Long", "(J)V", &[V::Long(v)])?,
        Ret::Float(v) => env.new_object("java/lang/Float", "(F)V", &[V::Float(v)])?,
        Ret::Double(v) => env.new_object("java/lang/Double", "(D)V", &[V::Double(v)])?,
        Ret::Str(v) => env.new_string(v)?.into(),
        Ret::Obj(v) => env.new_local_ref(v.as_obj())?,
    })
}

/// `RUSTMC_FORGE_TRACE=1` prints every stubbed call as it happens (handled or not).
fn trace() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RUSTMC_FORGE_TRACE").is_some())
}

extern "system" fn dispatch<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, key: JString<'l>, this: JObject<'l>, args: JObjectArray<'l>) -> JObject<'l> {
    let key: String = match env.get_string(&key) { Ok(k) => k.into(), Err(_) => return JObject::null() };
    if trace() { eprintln!("[forge] call {key}"); }
    let Some(handler) = resolve(&key) else { return JObject::null() };
    let result = handler(&mut Call { env: &mut env, this, args }).and_then(|ret| to_java(&mut env, ret));
    if trace() { eprintln!("[forge] done {key}"); }
    result.unwrap_or_else(|e| {
        eprintln!("[forge] stub handler {key} failed: {e}");
        let _ = env.exception_describe(); // prints the Java stack trace and clears it
        JObject::null()
    })
}

/// `class` is `rustmc.forge.Stub` from the Forge-visible class loader. Until this runs, stubs return zero silently.
pub fn bind(env: &mut JNIEnv<'_>, class: &JClass<'_>) -> Result<()> {
    let methods = [NativeMethod {
        name: "dispatch".into(),
        sig: "(Ljava/lang/String;Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;".into(),
        fn_ptr: dispatch as *mut c_void,
    }];
    env.register_native_methods(class, &methods)?;
    env.call_static_method(class, "bind", "()V", &[])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_exact_then_short_key_and_records_the_rest() {
        register("a/B.done()V", |_| Ok(Ret::Null));
        register("a/B.any", |_| Ok(Ret::Null));
        assert!(resolve("a/B.done()V").is_some());
        assert!(resolve("a/B.any(I)I").is_some());
        assert!(resolve("a/B.todo()V").is_none());
        assert!(stubbed().contains(&"a/B.todo()V".to_string()));
        assert!(!stubbed().contains(&"a/B.any(I)I".to_string()));
    }
}
