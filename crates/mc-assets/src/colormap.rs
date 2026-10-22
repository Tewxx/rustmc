//! Biome color maps (`ColorizerGrass` / `ColorizerFoliage`).

use crate::AssetSource;

pub struct ColorMap {
    pixels: Vec<u32>,
}

impl ColorMap {
    /// `name` is `grass` or `foliage`.
    pub fn load(assets: &AssetSource, name: &str) -> Option<Self> {
        let img = image::load_from_memory(&assets.read(&format!("textures/colormap/{name}.png"))?).ok()?.to_rgba8();
        if img.width() != 256 || img.height() != 256 { return None; }
        let pixels = img.pixels().map(|p| u32::from_be_bytes([p[3], p[0], p[1], p[2]])).collect();
        Some(Self { pixels })
    }

    /// Vanilla lookup: rain is scaled by temperature, then both index a 256×256 triangle.
    pub fn get(&self, temperature: f64, rainfall: f64) -> u32 {
        let rainfall = rainfall * temperature;
        let i = ((1.0 - temperature) * 255.0) as i32;
        let j = ((1.0 - rainfall) * 255.0) as i32;
        let k = (j << 8) | i;
        if k < 0 || k as usize >= self.pixels.len() {
            0xFF00FF
        } else {
            self.pixels[k as usize]
        }
    }
}
