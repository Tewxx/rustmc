//! Port of `EntityRenderer.updateLightmap` for the overworld (no night vision,
//! lightning or boss fog). Index `sky * 16 + block`, RGBA8.

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

/// `torch_flicker` is `torchFlickerX` (random, ~0 on average). `gamma` is the
/// brightness option: 0 = Moody (vanilla default), 1 = Bright.
pub fn build(sun: f32, torch_flicker: f32, gamma: f32) -> [u8; 256 * 4] {
    let table = brightness_table();
    let mut out = [0u8; 256 * 4];
    let f1 = sun * 0.95 + 0.05;
    for i in 0..256 {
        let f2 = table[i / 16] * f1;
        let f3 = table[i % 16] * (torch_flicker * 0.1 + 1.5);
        let f4 = f2 * (sun * 0.65 + 0.35);
        let f5 = f2 * (sun * 0.65 + 0.35);
        let f6 = f3 * ((f3 * 0.6 + 0.4) * 0.6 + 0.4);
        let f7 = f3 * (f3 * f3 * 0.6 + 0.4);
        let mut rgb = [f4 + f3, f5 + f6, f2 + f7];
        for c in &mut rgb {
            *c = (*c * 0.96 + 0.03).min(1.0);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noon_full_sky_is_near_white_and_dark_is_dark() {
        assert_eq!(sun_brightness(0.0), 1.0);
        let lm = build(1.0, 0.0, 0.0);
        let full_sky = &lm[(15 * 16) * 4..(15 * 16) * 4 + 3];
        assert!(full_sky.iter().all(|&c| c > 240), "{full_sky:?}");
        let dark = &lm[0..3];
        assert!(dark.iter().all(|&c| c < 20), "{dark:?}");
    }
}
