//! Overworld sky values and geometry from WorldProvider/World/RenderGlobal in 1.8.9.
use mc_core::math;

#[derive(Clone, Debug)]
pub struct Sky {
    pub angle: f32,
    pub sky: [f32; 3],
    pub fog: [f32; 3],
    pub cloud: [f32; 3],
    pub sunrise: Option<[f32; 4]>,
    pub stars: f32,
    pub celestial_visibility: f32,
    pub moon: u8,
    pub ticks: f32,
    pub fancy_clouds: bool,
    pub clouds: bool,
}
impl Sky {
    /// Rain and thunder darkening of `World.getSkyColor`, `EntityRenderer.updateFogColor` and
    /// `World.getCloudColour`.
    pub fn apply_weather(&mut self, rain: f32, thunder: f32) {
        let lum = |c: &[f32; 3]| c[0] * 0.3 + c[1] * 0.59 + c[2] * 0.11;
        self.celestial_visibility = 1. - rain;
        self.stars *= self.celestial_visibility;
        if rain > 0. {
            let g = lum(&self.sky) * 0.6;
            let k = 1. - rain * 0.75;
            self.sky = self.sky.map(|c| c * k + g * (1. - k));
            let g = lum(&self.cloud) * 0.6;
            let k = 1. - rain * 0.95;
            self.cloud = self.cloud.map(|c| c * k + g * (1. - k));
            let (a, b) = (1. - rain * 0.5, 1. - rain * 0.4);
            self.fog = [self.fog[0] * a, self.fog[1] * a, self.fog[2] * b];
        }
        if thunder > 0. {
            let g = lum(&self.sky) * 0.2;
            let k = 1. - thunder * 0.75;
            self.sky = self.sky.map(|c| c * k + g * (1. - k));
            let g = lum(&self.cloud) * 0.2;
            let k = 1. - thunder * 0.95;
            self.cloud = self.cloud.map(|c| c * k + g * (1. - k));
            let a = 1. - thunder * 0.5;
            self.fog = self.fog.map(|c| c * a);
        }
    }

    pub fn overworld(time: i64, partial: f32, temperature: f32, distance_chunks: i32) -> Self {
        let mut angle = (((time % 24000) as i32) as f32 + partial) / 24000. - 0.25;
        if angle < 0. {
            angle += 1.;
        }
        if angle > 1. {
            angle -= 1.;
        }
        // WorldProvider.calculateCelestialAngle, including the f32 narrowing. Its "1/3 blend" is
        // `f = f + (f - f) / 3.0F` in 1.8.9: a no-op.
        angle = 1. - (((angle as f64 * std::f64::consts::PI).cos() + 1.) / 2.) as f32;
        let daylight = (math::cos(angle * std::f32::consts::PI * 2.) * 2. + 0.5).clamp(0., 1.);
        let temp = (temperature / 3.).clamp(-1., 1.);
        let rgb = math::hsv_to_rgb(0.62222224 - temp * 0.05, 0.5 + temp * 0.1, 1.) as u32;
        let sky = [
            (rgb >> 16 & 255) as f32 / 255. * daylight,
            (rgb >> 8 & 255) as f32 / 255. * daylight,
            (rgb & 255) as f32 / 255. * daylight,
        ];
        let mut fog = [
            0.7529412 * (daylight * 0.94 + 0.06),
            0.84705883 * (daylight * 0.94 + 0.06),
            daylight * 0.91 + 0.09,
        ];
        let blend = 1. - (0.25 + 0.75 * distance_chunks as f32 / 32.).powf(0.25);
        for i in 0..3 {
            fog[i] += (sky[i] - fog[i]) * blend;
        }
        let c = math::cos(angle * std::f32::consts::PI * 2.);
        let sunrise = if (-0.4..=0.4).contains(&c) {
            let f = c / 0.4 * 0.5 + 0.5;
            let a = 1. - (1. - math::sin(f * std::f32::consts::PI)) * 0.99;
            Some([f * 0.3 + 0.7, f * f * 0.7 + 0.2, 0.2, a * a])
        } else {
            None
        };
        let stars = (1. - (c * 2. + 0.25)).clamp(0., 1.);
        Self {
            angle,
            sky,
            fog,
            cloud: [daylight * 0.9 + 0.1, daylight * 0.9 + 0.1, daylight * 0.85 + 0.15],
            sunrise,
            stars: stars * stars * 0.5,
            celestial_visibility: 1.,
            moon: ((time / 24000 % 8 + 8) % 8) as u8,
            ticks: time as f32 + partial,
            fancy_clouds: true,
            clouds: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Sky;

    #[test]
    fn rain_fades_celestial_objects() {
        let mut sky = Sky::overworld(18000, 0., 0.8, 8);
        let clear_stars = sky.stars;
        assert!(clear_stars > 0.);
        sky.apply_weather(0.5, 0.);
        assert_eq!(sky.celestial_visibility, 0.5);
        assert_eq!(sky.stars, clear_stars * 0.5);
    }
}
