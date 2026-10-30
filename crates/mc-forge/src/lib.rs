//! JNI dispatch for the supported Forge events. The caller owns JVM startup and tick timing.
pub use jni;
pub mod game;
pub mod impls;
pub mod runtime;
pub mod stubs;

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

/// Rust-owned values mirrored to a Forge-loaded Minecraft MovementInput object.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MovementState {
    pub strafe: f32,
    pub forward: f32,
    pub jump: bool,
    pub sneak: bool,
}

/// One view per Rust owner; dropping it releases this bridge's JVM reference.
pub struct MovementView {
    object: GlobalRef,
}

impl MovementView {
    /// `class` must be MovementInput from the Forge LaunchClassLoader.
    pub fn new(env: &mut JNIEnv<'_>, class: JClass<'_>) -> Result<Self> {
        let object = env.new_object(class, "()V", &[])?;
        Ok(Self { object: env.new_global_ref(object)? })
    }

    pub fn object(&self) -> &GlobalRef { &self.object }

    /// Copy Rust state into the same Java object before mod callbacks.
    pub fn push(&self, env: &mut JNIEnv<'_>, state: MovementState) -> Result<()> {
        let obj = self.object.as_obj();
        env.set_field(obj, "field_78902_a", "F", JValue::Float(state.strafe))?;
        env.set_field(obj, "field_78900_b", "F", JValue::Float(state.forward))?;
        env.set_field(obj, "field_78901_c", "Z", JValue::Bool(state.jump.into()))?;
        env.set_field(obj, "field_78899_d", "Z", JValue::Bool(state.sneak.into()))?;
        Ok(())
    }

    /// Read all Java fields before the caller commits their values to Rust state.
    pub fn pull(&self, env: &mut JNIEnv<'_>) -> Result<MovementState> {
        let obj = self.object.as_obj();
        Ok(MovementState {
            strafe: env.get_field(obj, "field_78902_a", "F")?.f()?,
            forward: env.get_field(obj, "field_78900_b", "F")?.f()?,
            jump: env.get_field(obj, "field_78901_c", "Z")?.z()?,
            sneak: env.get_field(obj, "field_78899_d", "Z")?.z()?,
        })
    }
}
