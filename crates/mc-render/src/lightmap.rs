//! Port of `EntityRenderer.updateLightmap`. Index `sky * 16 + block`, RGBA8.

/// `WorldProvider.generateLightBrightnessTable` with no ambient light.
fn brightness_table() -> [f32; 16] {
    std::array::from_fn(|i| {
        let f1 = 1.0 - i as f32 / 15.0;
        (1.0 - f1) / (f1 * 3.0 + 1.0)
    })
}

/// `World.getSunBrightness` with no rain or thunder. `celestial_angle` 0 is noon.
pub fn sun_brightness(celestial_angle: f32) -> f32 {
    let f1 = 1.0 - (mc_core::math::cos(celestial_angle * std::f32::consts::PI * 2.0) * 2.0 + 0.2);
    let f1 = 1.0 - f1.clamp(0.0, 1.0);
    f1 * 0.8 + 0.2
}

/// `World.getSunBrightness` with rain and thunder strengths.
pub fn sun_brightness_weather(celestial_angle: f32, rain: f32, thunder: f32) -> f32 {
    let f1 = 1.0 - (mc_core::math::cos(celestial_angle * std::f32::consts::PI * 2.0) * 2.0 + 0.2);
    let mut f1 = 1.0 - f1.clamp(0.0, 1.0);
    f1 *= 1.0 - rain * 5.0 / 16.0;
    f1 *= 1.0 - thunder * 5.0 / 16.0;
    f1 * 0.8 + 0.2
}

/// `EntityRenderer.getNightVisionBrightness`: the brightening factor for the remaining
/// `Potion.nightVision` duration.
pub fn night_vision_brightness(duration: i32, partial_ticks: f32) -> f32 {
    if duration > 200 {
        1.0
    } else {
        0.7 + mc_core::math::sin((duration as f32 - partial_ticks) * std::f32::consts::PI * 0.2) * 0.3
    }
}

/// `torch_flicker` is `torchFlickerX` (random, ~0 on average). `gamma` is the
/// brightness option: 0 = Moody (vanilla default), 1 = Bright. `dimension` is the
/// provider id: 1 (the End) overrides the ambient colour, -1 (the Nether) shares the
/// fixed `1.0` sun brightness but needs no override. `night_vision` is the
/// [`night_vision_brightness`] factor, and `lightning` is `world.getLastLightningBolt() > 0`.
pub fn build(sun: f32, torch_flicker: f32, gamma: f32, dimension: i32, night_vision: Option<f32>, lightning: bool) -> [u8; 256 * 4] {
    let table = brightness_table();
    let mut out = [0u8; 256 * 4];
    let f1 = sun * 0.95 + 0.05;
    for i in 0..256 {
        let mut f2 = table[i / 16] * f1;
        let f3 = table[i % 16] * (torch_flicker * 0.1 + 1.5);
        // `if (world.getLastLightningBolt() > 0) f2 = table[i / 16];`
        if lightning {
            f2 = table[i / 16];
        }
        let f4 = f2 * (sun * 0.65 + 0.35);
        let f5 = f2 * (sun * 0.65 + 0.35);
        let f6 = f3 * ((f3 * 0.6 + 0.4) * 0.6 + 0.4);
        let f7 = f3 * (f3 * f3 * 0.6 + 0.4);
        let mut rgb = [f4 + f3, f5 + f6, f2 + f7];
        for c in &mut rgb {
            *c = *c * 0.96 + 0.03;
        }
        // `WorldProviderEnd`: a fixed, block-light-driven palette replaces the sky term.
        if dimension == 1 {
            rgb = [0.22 + f3 * 0.75, 0.28 + f6 * 0.75, 0.25 + f7 * 0.75];
        }
        if let Some(f15) = night_vision {
            let mut f12 = 1.0 / rgb[0];
            if f12 > 1.0 / rgb[1] {
                f12 = 1.0 / rgb[1];
            }
            if f12 > 1.0 / rgb[2] {
                f12 = 1.0 / rgb[2];
            }
            for c in &mut rgb {
                *c = *c * (1.0 - f15) + *c * f12 * f15;
            }
        }
        for c in &mut rgb {
            // Java's `if (f8 > 1.0F)` leaves NaN alone (night vision over a black texel); a
            // float `min` would replace it with 1.0.
            if *c > 1.0 {
                *c = 1.0;
            }
            let inv = 1.0 - *c;
            let bright = 1.0 - inv * inv * inv * inv;
            *c = *c * (1.0 - gamma) + bright * gamma;
            *c = (*c * 0.96 + 0.03).clamp(0.0, 1.0);
        }
        out[i * 4] = (rgb[0] * 255.0) as u8;
        out[i * 4 + 1] = (rgb[1] * 255.0) as u8;
        out[i * 4 + 2] = (rgb[2] * 255.0) as u8;
        out[i * 4 + 3] = 255;
    }
    out
}

/// One `GL_LINEAR` lightmap sample at the packed-light coordinates `sky`/`block`, each in the
/// `0..240` units the chunk shader feeds to `(coord + 8) / 256`. The usual integer values land
/// on texel centres; `EntityXPOrb.getBrightnessForRender`'s `+120` falls halfway between two
/// rows and blends, so the lightmap must be filtered and not nearest-sampled.
pub fn sample(rgba: &[u8; 256 * 4], sky: f32, block: f32) -> [f32; 3] {
    let texel = |u: f32| ((u + 8.0) / 16.0 - 0.5).clamp(0.0, 15.0);
    let (tb, ts) = (texel(block), texel(sky));
    let (b0, s0) = (tb.floor(), ts.floor());
    let (fb, fs) = (tb - b0, ts - s0);
    let (b0, s0) = (b0 as usize, s0 as usize);
    let (b1, s1) = ((b0 + 1).min(15), (s0 + 1).min(15));
    let at = |b: usize, s: usize| &rgba[(s * 16 + b) * 4..][..3];
    std::array::from_fn(|c| {
        let a = at(b0, s0)[c] as f32 * (1.0 - fb) + at(b1, s0)[c] as f32 * fb;
        let b = at(b0, s1)[c] as f32 * (1.0 - fb) + at(b1, s1)[c] as f32 * fb;
        (a * (1.0 - fs) + b * fs) / 255.0
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noon_full_sky_is_near_white_and_dark_is_dark() {
        assert_eq!(sun_brightness(0.0), 1.0);
        let lm = build(1.0, 0.0, 0.0, 0, None, false);
        let full_sky = &lm[(15 * 16) * 4..(15 * 16) * 4 + 3];
        assert!(full_sky.iter().all(|&c| c > 240), "{full_sky:?}");
        let dark = &lm[0..3];
        assert!(dark.iter().all(|&c| c < 20), "{dark:?}");
    }

    #[test]
    fn end_palette_ignores_the_sky_term() {
        let lm = build(1.0, 0.0, 0.0, 1, None, false);
        // Sky light alone (block 0, sky 15) keeps the fixed end palette.
        let sky_only = &lm[(15 * 16) * 4..(15 * 16) * 4 + 3];
        assert_eq!(sky_only, &[sfx(0.22), sfx(0.28), sfx(0.25)]);
        // Block light 15 saturates; the end's `0.22 + 1.125` clamps to 1 before the final
        // 0.96/0.03 pass, so the brightest texel is 252 rather than 255.
        assert_eq!(&lm[(15 * 16 + 15) * 4..][..3], &[252, 252, 252]);
    }

    fn sfx(v: f32) -> u8 {
        ((v * 0.96 + 0.03) * 255.0) as u8
    }

    #[test]
    fn lightning_drops_the_sun_factor() {
        let with = build(0.2, 0.0, 0.0, 0, None, true);
        let without = build(0.2, 0.0, 0.0, 0, None, false);
        let sky_texel = |lm: &[u8; 1024]| lm[(15 * 16) * 4 + 2];
        assert!(sky_texel(&with) > sky_texel(&without));
    }

    #[test]
    fn night_vision_brightens_the_darkest_texels() {
        let normal = build(0.0, 0.0, 0.0, 0, None, false);
        let nv = build(0.0, 0.0, 0.0, 0, Some(1.0), false);
        assert!(nv[4] > normal[4], "torch level 1 texel brightens");
        assert_eq!(night_vision_brightness(400, 0.0), 1.0);
        assert!((night_vision_brightness(0, 0.0) - 0.7).abs() < 1e-6);
    }

    #[test]
    fn sample_hits_texel_centres_and_blends_between_rows() {
        let lm = build(1.0, 0.0, 0.0, 0, None, false);
        // Full bright is 252 after vanilla's final `* 0.96 + 0.03` pass.
        assert_eq!(sample(&lm, 240.0, 240.0), [252.0 / 255.0; 3]);
        // The XP orb bump: block light 0 + 120 units sits halfway between levels 7 and 8.
        let a = sample(&lm, 0.0, 7.0 * 16.0);
        let b = sample(&lm, 0.0, 8.0 * 16.0);
        let mid = sample(&lm, 0.0, 120.0);
        for c in 0..3 {
            assert!((mid[c] - (a[c] + b[c]) * 0.5).abs() < 1e-5, "{c}");
        }
    }
}
