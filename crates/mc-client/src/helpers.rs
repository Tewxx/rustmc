//! Small free helpers shared by the client modules.

use crate::*;

/// `String.hashCode`, used for non-numeric seeds like vanilla's world creation screen.
/// Entity flags (index 0): sneaking 0x02, sprinting 0x08.
pub(crate) fn apply_meta(r: &mut entities::Remote, meta: &mc_protocol::Metadata) {
    for (i, v) in meta {
        r.meta.insert(*i, v.clone());
        if let (0, mc_protocol::MetaValue::Byte(b)) = (i, v) {
            r.sneaking = b & 2 != 0;
            r.sprinting = b & 8 != 0;
            r.eating = b & 16 != 0;
        }
    }
}

/// `RendererLivingEntity.interpolateRotation`: the short way around.
pub(crate) fn lerp_angle(a: f32, b: f32, pt: f32) -> f32 {
    let mut f = b - a;
    while f < -180. {
        f += 360.;
    }
    while f >= 180. {
        f -= 360.;
    }
    a + pt * f
}

pub(crate) fn java_string_hash(s: &str) -> i32 {
    s.encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
}

/// `PacketBuffer.readStringFromBuffer`: VarInt byte length, then UTF-8.
pub(crate) fn packet_string(data: &[u8]) -> String {
    let mut i = 0;
    let mut len = 0u32;
    let mut shift = 0;
    while i < data.len() && shift < 35 {
        let b = data[i];
        i += 1;
        len |= ((b & 0x7f) as u32) << shift;
        shift += 7;
        if b & 0x80 == 0 {
            break;
        }
    }
    let n = (len as usize).min(data.len().saturating_sub(i));
    String::from_utf8_lossy(&data[i..i + n]).into_owned()
}

/// Emits a baked block model with `base` applied, shading each quad by its vanilla face
/// brightness and the entity's lightmap colour.
pub(crate) fn push_model_quads(out: &mut Vec<mc_render::avatar::WorldQuad>, base: glam::Mat4, model: &mc_assets::model::BakedModel, light: [f32; 3]) {
    for q in &model.quads {
        out.push(mc_render::avatar::WorldQuad {
            pos: q.pos.map(|p| base.transform_point3(Vec3::from(p))),
            uv: q.uv,
            color: [q.shade * light[0], q.shade * light[1], q.shade * light[2]],
        });
    }
}

/// `EntityList` name → `entity.<Name>.name`, for the default name tag of a mob that has
/// `customNameVisible` set but no custom name.
pub(crate) fn mob_lang_key(kind: u8) -> &'static str {
    match kind {
        50 => "entity.Creeper.name",
        51 => "entity.Skeleton.name",
        52 => "entity.Spider.name",
        53 => "entity.Giant.name",
        54 => "entity.Zombie.name",
        55 => "entity.Slime.name",
        56 => "entity.Ghast.name",
        57 => "entity.PigZombie.name",
        58 => "entity.Enderman.name",
        59 => "entity.CaveSpider.name",
        60 => "entity.Silverfish.name",
        61 => "entity.Blaze.name",
        62 => "entity.LavaSlime.name",
        63 => "entity.EnderDragon.name",
        64 => "entity.WitherBoss.name",
        65 => "entity.Bat.name",
        66 => "entity.Witch.name",
        67 => "entity.Endermite.name",
        68 => "entity.Guardian.name",
        90 => "entity.Pig.name",
        91 => "entity.Sheep.name",
        92 => "entity.Cow.name",
        93 => "entity.Chicken.name",
        94 => "entity.Squid.name",
        95 => "entity.Wolf.name",
        96 => "entity.MushroomCow.name",
        97 => "entity.SnowMan.name",
        98 => "entity.Ozelot.name",
        99 => "entity.VillagerGolem.name",
        100 => "entity.EntityHorse.name",
        101 => "entity.Rabbit.name",
        120 => "entity.Villager.name",
        _ => "entity.Mob.name",
    }
}

/// `GuiScreen.openWebLink`: hands the link to the desktop's browser (`Desktop.browse`).
pub(crate) fn open_web_link(url: &str) {
    #[cfg(windows)]
    let r = std::process::Command::new("rundll32").arg("url.dll,FileProtocolHandler").arg(url).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    if let Err(e) = r {
        eprintln!("Couldn't open link: {e}");
    }
}

/// `Math.random()`
pub(crate) fn rand_f64() -> f64 {
    use std::cell::Cell;
    thread_local!(static STATE: Cell<u64> = Cell::new(0x2545F4914F6CDD1D ^ std::process::id() as u64));
    STATE.with(|s| {
        let mut x = s.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        (x >> 11) as f64 / (1u64 << 53) as f64
    })
}
