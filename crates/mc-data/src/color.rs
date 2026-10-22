//! Block color methods and `BiomeColorHelper` (RGB channel averaging, before shading).

use crate::{ids, Block, BlockAccess, Material, StateId};
use mc_core::BlockPos;

pub trait Colorizer {
    fn grass(&self, temperature: f64, rainfall: f64) -> u32;
    fn foliage(&self, temperature: f64, rainfall: f64) -> u32;
}

/// Unblended biome colors at a position. Tall grass, reeds and vines deliberately use a single
/// sample; grass blocks, leaves, double plants and water use the 3x3 `BiomeColorHelper` average.
pub trait ColorAccess: BlockAccess {
    fn grass_color(&self, pos: BlockPos) -> u32;
    fn foliage_color(&self, pos: BlockPos) -> u32;
    fn water_color(&self, pos: BlockPos) -> u32;
}

pub fn blend(pos: BlockPos, sample: impl Fn(BlockPos) -> u32) -> u32 {
    let (mut r, mut g, mut b) = (0, 0, 0);
    for z in -1..=1 {
        for x in -1..=1 {
            let c = sample(pos.offset(x, 0, z));
            r += c >> 16 & 255;
            g += c >> 8 & 255;
            b += c & 255;
        }
    }
    (r / 9) << 16 | (g / 9) << 8 | b / 9
}

impl Block {
    pub fn block_color(&self, colors: &impl Colorizer) -> u32 {
        match self.class {
            "BlockGrass" | "BlockTallGrass" => colors.grass(0.5, 1.0),
            "BlockOldLeaf" | "BlockNewLeaf" => colors.foliage(0.5, 1.0),
            "BlockVine" => 4_764_952,
            "BlockLilyPad" => 7_455_580,
            _ => 0xFFFFFF,
        }
    }

    /// `colorMultiplier`. The receiver can differ from the block at `pos` (potted plants).
    pub fn color_multiplier(&self, world: &impl ColorAccess, pos: BlockPos) -> u32 {
        match self.class {
            "BlockGrass" => blend(pos, |p| world.grass_color(p)),
            "BlockOldLeaf" if world.state(pos).block().id == self.id => match world.state(pos).expect("variant") {
                "spruce" => 6_396_257,
                "birch" => 8_431_445,
                _ => blend(pos, |p| world.foliage_color(p)),
            },
            "BlockOldLeaf" | "BlockNewLeaf" => blend(pos, |p| world.foliage_color(p)),
            "BlockTallGrass" | "BlockReed" => world.grass_color(pos),
            "BlockVine" => world.foliage_color(pos),
            "BlockLilyPad" => 2_129_968,
            "BlockDoublePlant" => {
                let s = world.state(pos);
                let variant = if s.block().id == self.id { s.actual_state(world, pos).expect("variant") } else { "double_fern" };
                if matches!(variant, "double_grass" | "double_fern") {
                    blend(pos, |p| world.grass_color(p))
                } else {
                    0xFFFFFF
                }
            }
            "BlockStem" => {
                let s = world.state(pos);
                if s.block().id == self.id {
                    stem_color(s.get_int("age"))
                } else {
                    0xFFFFFF
                }
            }
            "BlockRedstoneWire" => {
                let s = world.state(pos);
                if s.block().id == self.id {
                    redstone_color(s.get_int("power"))
                } else {
                    0xFFFFFF
                }
            }
            "BlockDynamicLiquid" | "BlockStaticLiquid" if self.material == Material::Water => blend(pos, |p| world.water_color(p)),
            "BlockFlowerPot" => match world.flower_pot(pos).and_then(|(item, _)| u16::try_from(item).ok()).and_then(Block::by_id) {
                Some(b) if b.id != ids::AIR && b.id != ids::FLOWER_POT => b.color_multiplier(world, pos),
                _ => 0xFFFFFF,
            },
            _ => 0xFFFFFF,
        }
    }
}

impl StateId {
    pub fn render_color(self, colors: &impl Colorizer) -> u32 {
        match self.block().class {
            "BlockGrass" => self.block().block_color(colors),
            "BlockOldLeaf" => match self.expect("variant") {
                "spruce" => 6_396_257,
                "birch" => 8_431_445,
                _ => 4_764_952,
            },
            "BlockNewLeaf" | "BlockVine" => 4_764_952,
            "BlockLilyPad" => 7_455_580,
            "BlockTallGrass" if self.expect("type") != "dead_bush" => colors.grass(0.5, 1.0),
            "BlockStem" => stem_color(self.get_int("age")),
            _ => 0xFFFFFF,
        }
    }
}

fn stem_color(age: i32) -> u32 {
    ((age * 32) << 16 | (255 - age * 8) << 8 | age * 4) as u32
}

fn redstone_color(power: i32) -> u32 {
    let f = power as f32 / 15.0;
    let r = if power == 0 { 0.3 } else { f * 0.6 + 0.4 };
    let g = (f * f * 0.7 - 0.5).max(0.0);
    let b = (f * f * 0.6 - 0.7).max(0.0);
    let ch = |v: f32| (v * 255.0) as u32;
    0xFF000000 | ch(r) << 16 | ch(g) << 8 | ch(b)
}
