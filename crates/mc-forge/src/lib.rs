//! JNI dispatch for the supported Forge events. The caller owns JVM startup and tick timing.
use jni::{errors::Result, objects::{GlobalRef, JClass, JStaticMethodID, JValue}, signature::{Primitive, ReturnType}, JNIEnv};

pub struct ClientTickBridge {
    class: GlobalRef,
    post: JStaticMethodID,
}

impl ClientTickBridge {
    /// `class` must be the Forge-loaded rustmc.forge.EventBridge class, not a system-loader copy.
    pub fn new(env: &mut JNIEnv<'_>, class: JClass<'_>) -> Result<Self> {
        let post = env.get_static_method_id(&class, "clientTick", "(Z)V")?;
        Ok(Self { class: env.new_global_ref(class)?, post })
    }

    pub fn post(&self, env: &mut JNIEnv<'_>, start: bool) -> Result<()> {
        let class: &JClass = self.class.as_obj().into();
        let args = [JValue::Bool(start.into()).as_jni()];
        // The class, method ID and (Z)V signature were resolved together in new().
        unsafe { env.call_static_method_unchecked(class, self.post, ReturnType::Primitive(Primitive::Void), &args) }?;
        Ok(())
    }
}
