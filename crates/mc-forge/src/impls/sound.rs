//! The sound API: `SoundHandler`/`SoundManager`, `PositionedSoundRecord`, the `ISound` getters and
//! `SoundEventAccessorComposite`. Everything Java code can observe lives in `rustmc.forge.Sounds` (the stub
//! constructors never run, so object graphs there are wired by hand); playback requests queue up here.
use crate::{runtime as rt, stubs::{register, Call, Ret}};
use jni::{errors::Result, objects::{GlobalRef, JDoubleArray, JObject, JValue}, JNIEnv};
use std::sync::{Mutex, OnceLock};

const SOUNDS: &str = "rustmc/forge/Sounds";
const HANDLER: &str = "net/minecraft/client/audio/SoundHandler";
const MANAGER: &str = "net/minecraft/client/audio/SoundManager";
const RECORD: &str = "net/minecraft/client/audio/PositionedSoundRecord";
const POSITIONED: &str = "net/minecraft/client/audio/PositionedSound";
const COMPOSITE: &str = "net/minecraft/client/audio/SoundEventAccessorComposite";
static INSTANCE: OnceLock<GlobalRef> = OnceLock::new();

/// One playback request for the client's mixer.
#[derive(Clone, Debug, PartialEq)]
pub struct SoundRequest {
    /// `domain:path` as `ResourceLocation` spells it (`minecraft:random.click`); see [`Self::event`].
    pub location: String,
    pub volume: f32,
    pub pitch: f32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    /// `ISound.AttenuationType#getType()`: 0 = `NONE` (GUI sound), 2 = `LINEAR` (heard in the world).
    pub attenuation: i32,
    pub repeat: bool,
}

impl SoundRequest {
    /// The name the mixer indexes sounds.json by: the path part of the resource location.
    pub fn event(&self) -> &str {
        let path = self.location.split_once(':').map_or(self.location.as_str(), |(_, path)| path);
        if path.is_empty() { &self.location } else { path }
    }

    /// Where the mixer places the sound; `NONE` attenuation is heard at a constant volume.
    pub fn position(&self) -> Option<[f64; 3]> {
        (self.attenuation != 0).then(|| [self.x as f64, self.y as f64, self.z as f64])
    }
}

fn queue() -> &'static Mutex<Vec<SoundRequest>> {
    static QUEUE: OnceLock<Mutex<Vec<SoundRequest>>> = OnceLock::new();
    QUEUE.get_or_init(Default::default)
}

/// Drain the sounds mods played since the last call; the client hands them to its mixer.
pub fn take_sounds() -> Vec<SoundRequest> {
    std::mem::take(&mut *queue().lock().unwrap())
}

pub(crate) fn register_all() {
    register("net/minecraft/client/Minecraft.func_147118_V", |c| {
        let handler = super::singleton(c.env, &INSTANCE, HANDLER)?;
        rt::call_static(c.env, SOUNDS, "init", "(Lnet/minecraft/client/audio/SoundHandler;)V",
            &[JValue::Object(&handler)])?;
        Ok(Ret::Obj(c.env.new_global_ref(handler)?))
    });
    // Playback needs the Rust queue; the playing set stays in Sounds.
    for key in [format!("{HANDLER}.func_147682_a"), format!("{HANDLER}.func_147681_a")] {
        register(&key, |c| play(c));
    }
    // `SoundManager.playSound` and `stopSound` share the descriptor `(ISound)V`, so decide by the playing set.
    for key in [format!("{MANAGER}.func_148602_b"), format!("{MANAGER}.func_148611_c")] {
        register(&key, |c| manager_sound(c));
    }
    // The rest only moves Java-visible state (records, events, the playing set, levels).
    for (class, methods) in [
        (RECORD, &["func_147673_a", "func_147674_a", "func_147675_a"][..]),
        (POSITIONED, &["func_147650_b", "func_147657_c", "func_147652_d", "func_147653_e", "func_147655_f",
            "func_147649_g", "func_147654_h", "func_147651_i", "func_147656_j"][..]),
        (COMPOSITE, &["func_148729_c", "func_148728_d", "func_148721_a", "func_148720_g", "func_148727_a"][..]),
        (HANDLER, &["func_147680_a", "func_147683_b", "func_147692_c", "func_147689_b", "func_147687_e",
            "func_147690_c", "func_147685_d", "func_147686_a", "func_147684_a", "func_147691_a", "func_73660_a",
            "func_110549_a", "func_175085_a"][..]),
        (MANAGER, &["func_148597_a", "func_148605_d", "func_148604_f", "func_148608_i", "func_148596_a",
            "func_148613_b", "func_148610_e", "func_148595_a", "func_148601_a", "func_148615_a"][..]),
    ] {
        for &method in methods {
            let key = format!("{class}.{method}");
            let route = key.clone();
            register(&key, move |c| route_to_java(c, &route));
            // Same SRG name = the same virtual method in a subclass; mods override these.
            let wild = format!("*.{method}");
            register(&wild, move |c| route_to_java(c, &key));
        }
    }
}

fn route_to_java(c: &mut Call<'_, '_>, key: &str) -> Result<Ret> {
    let name = c.env.new_string(key)?;
    let this = c.env.new_local_ref(&c.this)?;
    let args = JObject::from(c.env.new_local_ref(c.arguments())?);
    let result = rt::call_static(c.env, SOUNDS, "invoke",
        "(Ljava/lang/String;Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;",
        &[JValue::Object(&name), JValue::Object(&this), JValue::Object(&args)])?.l()?;
    if result.is_null() { Ok(Ret::Null) } else { Ok(Ret::Obj(c.env.new_global_ref(result)?)) }
}

fn play(c: &mut Call<'_, '_>) -> Result<Ret> {
    let sound = c.obj(0)?;
    if sound.is_null() { return Ok(Ret::Null); } // vanilla ignores a null ISound
    let request = describe(c.env, &sound)?;
    queue().lock().unwrap().push(request);
    rt::call_static(c.env, SOUNDS, "mark", "(Ljava/lang/Object;)V", &[JValue::Object(&sound)])?;
    Ok(Ret::Null)
}

fn manager_sound(c: &mut Call<'_, '_>) -> Result<Ret> {
    let sound = c.obj(0)?;
    if sound.is_null() { return Ok(Ret::Null); }
    if !rt::call_static(c.env, SOUNDS, "playing", "(Ljava/lang/Object;)Z", &[JValue::Object(&sound)])?.z()? {
        return play(c);
    }
    rt::call_static(c.env, SOUNDS, "stop", "(Ljava/lang/Object;)V", &[JValue::Object(&sound)])?;
    Ok(Ret::Null)
}

fn describe(env: &mut JNIEnv<'_>, sound: &JObject<'_>) -> Result<SoundRequest> {
    let sig = "Lnet/minecraft/client/audio/ISound;";
    let location = rt::call_static(env, SOUNDS, "location", &format!("({sig})Ljava/lang/String;"),
        &[JValue::Object(sound)])?.l()?;
    let location: String = env.get_string(&location.into())?.into();
    let params = rt::call_static(env, SOUNDS, "params", &format!("({sig})[D"), &[JValue::Object(sound)])?.l()?;
    let mut p = [0f64; 7];
    env.get_double_array_region(&JDoubleArray::from(params), 0, &mut p)?;
    Ok(SoundRequest {
        location,
        volume: p[0] as f32,
        pitch: p[1] as f32,
        x: p[2] as f32,
        y: p[3] as f32,
        z: p[4] as f32,
        attenuation: p[5] as i32,
        repeat: p[6] != 0.,
    })
}

#[cfg(test)]
mod tests {
    use super::SoundRequest;

    fn request(location: &str, attenuation: i32) -> SoundRequest {
        SoundRequest {
            location: location.into(), volume: 1., pitch: 1., x: 1., y: 2., z: 3., attenuation, repeat: false,
        }
    }

    #[test]
    fn event_drops_the_namespace() {
        assert_eq!(request("minecraft:random.click", 0).event(), "random.click");
        assert_eq!(request("meowtils:crit", 2).event(), "crit");
        assert_eq!(request("", 0).event(), "");
    }

    #[test]
    fn attenuation_decides_whether_the_sound_has_a_position() {
        assert_eq!(request("minecraft:random.click", 0).position(), None);
        assert_eq!(request("minecraft:mob.cat.meow", 2).position(), Some([1., 2., 3.]));
    }
}
