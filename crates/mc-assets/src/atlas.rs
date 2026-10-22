//! Block texture atlas (`TextureMap`). One frame per sprite for now; animation comes later.

use crate::AssetSource;
use std::collections::HashMap;

pub const MISSING: &str = "missingno";

#[derive(Clone, Copy, Debug)]
pub struct Sprite {
    pub min_u: f32,
    pub min_v: f32,
    pub max_u: f32,
    pub max_v: f32,
}

impl Sprite {
    /// `getInterpolatedU`: `u` is in model texels (0–16).
    pub fn u(&self, u: f32) -> f32 {
        self.min_u + (self.max_u - self.min_u) * u / 16.0
    }

    pub fn v(&self, v: f32) -> f32 {
        self.min_v + (self.max_v - self.min_v) * v / 16.0
    }
}

pub struct Atlas {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    sprites: HashMap<String, Sprite>,
}

impl Atlas {
    /// Builds an atlas from texture names like `blocks/stone`. Missing or broken
    /// textures map to the magenta/black checker, as in vanilla.
    pub fn build(assets: &AssetSource, names: impl IntoIterator<Item = String>) -> Atlas {
        let mut frames: Vec<(String, image::RgbaImage)> = vec![(MISSING.to_string(), missing_texture())];
        let mut seen = std::collections::HashSet::new();
        for name in names {
            if !seen.insert(name.clone()) || name == MISSING {
                continue;
            }
            let Some(img) = assets
                .read(&format!("textures/{name}.png"))
                .and_then(|b| image::load_from_memory(&b).ok())
                .map(|i| i.to_rgba8())
            else {
                continue;
            };
            // Animated textures are vertical strips; the first square is frame 0.
            let w = img.width();
            let frame = image::imageops::crop_imm(&img, 0, 0, w, w.min(img.height())).to_image();
            frames.push((name, frame));
        }

        let cell = frames.iter().map(|(_, f)| f.width()).max().unwrap_or(16);
        let cols = (frames.len() as f32).sqrt().ceil() as u32;
        let rows = (frames.len() as u32).div_ceil(cols);
        let width = (cols * cell).next_power_of_two();
        let height = (rows * cell).next_power_of_two();
        let mut canvas = image::RgbaImage::new(width, height);
        let mut sprites = HashMap::new();
        for (i, (name, frame)) in frames.into_iter().enumerate() {
            let frame = if frame.width() == cell {
                frame
            } else {
                image::imageops::resize(&frame, cell, cell, image::imageops::FilterType::Nearest)
            };
            let (x, y) = ((i as u32 % cols) * cell, (i as u32 / cols) * cell);
            image::imageops::replace(&mut canvas, &frame, x as i64, y as i64);
            sprites.insert(
                name,
                Sprite {
                    min_u: x as f32 / width as f32,
                    min_v: y as f32 / height as f32,
                    max_u: (x + cell) as f32 / width as f32,
                    max_v: (y + cell) as f32 / height as f32,
                },
            );
        }
        Atlas { width, height, rgba: canvas.into_raw(), sprites }
    }

    pub fn sprite(&self, name: &str) -> Sprite {
        self.sprites.get(name).or_else(|| self.sprites.get(MISSING)).copied().unwrap()
    }
}

fn missing_texture() -> image::RgbaImage {
    image::RgbaImage::from_fn(16, 16, |x, y| {
        if (x < 8) ^ (y < 8) {
            image::Rgba([0, 0, 0, 255])
        } else {
            image::Rgba([248, 0, 248, 255])
        }
    })
}
