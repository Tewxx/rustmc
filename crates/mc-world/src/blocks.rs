//! 1.8.9 numeric block ids → blockstate file + variant key, plus render properties.
//!
//! Vanilla derives these from `Block.registerBlocks` and each block's state mapper.
//! This table covers common terrain and building blocks; unknown ids render nothing.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tint {
    None,
    Grass,
    Foliage,
    /// Fixed leaf colors (`ColorizerFoliage.getFoliageColorPine` / `getFoliageColorBirch`).
    Fixed(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Solid,
    /// Alpha-tested (leaves, plants, grass overlay).
    Cutout,
    /// Not drawn yet (water, glass panes, stained glass).
    Translucent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockInfo {
    pub blockstate: String,
    pub variant: String,
    /// Full opaque cube: hides neighbours' faces (`isOpaqueCube`).
    pub opaque: bool,
    pub tint: Tint,
    pub layer: Layer,
    pub props: LightProps,
}

/// The `Block` fields lighting and AO read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LightProps {
    /// `isFullCube`
    pub full_cube: bool,
    /// `isBlockNormalCube` (material blocks movement and full cube): AO value 0.2 instead of 1.0.
    pub normal_cube: bool,
    /// `isTranslucent` (`!material.blocksLight()`): lets AO see around corners.
    pub translucent: bool,
    /// `getLightOpacity`
    pub opacity: u8,
    /// `getLightValue`
    pub emission: u8,
}

impl LightProps {
    pub const AIR: LightProps = LightProps { full_cube: false, normal_cube: false, translucent: true, opacity: 0, emission: 0 };
    const SOLID: LightProps = LightProps { full_cube: true, normal_cube: true, translucent: false, opacity: 255, emission: 0 };
    const PLANT: LightProps = LightProps { full_cube: false, normal_cube: false, translucent: true, opacity: 0, emission: 0 };

    /// `getAmbientOcclusionLightValue`
    pub fn ao_value(&self) -> f32 {
        if self.normal_cube { 0.2 } else { 1.0 }
    }
}

/// Lighting properties for any id; ids outside the table act like air.
pub fn props(id: u16, meta: u8) -> LightProps {
    if id == 0 {
        return LightProps::AIR;
    }
    lookup(id, meta).map_or(LightProps::AIR, |b| b.props)
}

const COLORS: [&str; 16] = [
    "white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray", "silver", "cyan", "purple", "blue",
    "brown", "green", "red", "black",
];
const WOODS: [&str; 6] = ["oak", "spruce", "birch", "jungle", "acacia", "dark_oak"];

fn solid(bs: &str) -> BlockInfo {
    BlockInfo {
        blockstate: bs.into(),
        variant: "normal".into(),
        opaque: true,
        tint: Tint::None,
        layer: Layer::Solid,
        props: LightProps::SOLID,
    }
}

/// Plants (`BlockBush`, material `plants`).
fn cutout(bs: &str, variant: &str) -> BlockInfo {
    BlockInfo {
        blockstate: bs.into(),
        variant: variant.into(),
        opaque: false,
        tint: Tint::None,
        layer: Layer::Cutout,
        props: LightProps::PLANT,
    }
}

fn emitting(mut b: BlockInfo, level: u8) -> BlockInfo {
    b.props.emission = level;
    b
}

fn log_axis(meta: u8) -> &'static str {
    match meta >> 2 {
        0 => "axis=y",
        1 => "axis=x",
        2 => "axis=z",
        _ => "axis=none",
    }
}

/// Fancy leaves: not opaque, but a full normal cube with opacity 1.
fn leaves(wood: &str, tint: Tint) -> BlockInfo {
    BlockInfo {
        tint,
        props: LightProps { opacity: 1, ..LightProps::SOLID },
        ..cutout(&format!("{wood}_leaves"), "normal")
    }
}

/// Metas that `getMetaFromState` produces for the blocks in this table, as a bit mask.
/// Chunks store raw `id << 4 | meta`, but `ExtendedBlockStorage.get` resolves only these
/// (`Block.BLOCK_STATE_IDS`); any other pair reads as air.
fn registered_metas(id: u16) -> u16 {
    const fn upto(n: u16) -> u16 {
        (1 << (n + 1)) - 1
    }
    match id {
        1 => upto(6),
        3 | 24 | 31 | 168 => upto(2),
        5 => upto(5),
        12 | 19 => upto(1),
        17 | 18 | 35 | 159 => 0xFFFF,
        38 => upto(8),
        86 | 98 => upto(3),
        155 => upto(4),
        // Acacia/dark oak only: variant bits 0–1 of each axis/decay group.
        161 | 162 => 0x3333,
        170 => 1 | 1 << 4 | 1 << 8,
        _ => 1,
    }
}

pub fn lookup(id: u16, meta: u8) -> Option<BlockInfo> {
    if meta > 15 || registered_metas(id) & (1 << meta) == 0 {
        return None;
    }
    let m = meta as usize;
    Some(match id {
        1 => solid(["stone", "granite", "smooth_granite", "diorite", "smooth_diorite", "andesite", "smooth_andesite"].get(m)?),
        // BlockGrass renders in CUTOUT_MIPPED for its tinted side overlay.
        2 => BlockInfo { variant: "snowy=false".into(), tint: Tint::Grass, layer: Layer::Cutout, ..solid("grass") },
        3 => match meta {
            1 => solid("coarse_dirt"),
            2 => BlockInfo { variant: "snowy=false".into(), ..solid("podzol") },
            _ => solid("dirt"),
        },
        4 => solid("cobblestone"),
        5 => solid(&format!("{}_planks", WOODS.get(m)?)),
        7 => solid("bedrock"),
        12 => solid(if meta == 1 { "red_sand" } else { "sand" }),
        13 => solid("gravel"),
        14 => solid("gold_ore"),
        15 => solid("iron_ore"),
        16 => solid("coal_ore"),
        17 => BlockInfo { variant: log_axis(meta).into(), ..solid(&format!("{}_log", WOODS[m & 3])) },
        18 => match meta & 3 {
            1 => leaves("spruce", Tint::Fixed(0x619961)),
            2 => leaves("birch", Tint::Fixed(0x80A755)),
            w => leaves(WOODS[w as usize], Tint::Foliage),
        },
        19 => solid("sponge"),
        // BlockGlass: not a full cube, but its material blocks light.
        20 => BlockInfo { props: LightProps { translucent: false, ..LightProps::PLANT }, ..cutout("glass", "normal") },
        21 => solid("lapis_ore"),
        22 => solid("lapis_block"),
        24 => solid(["sandstone", "chiseled_sandstone", "smooth_sandstone"].get(m)?),
        31 => match meta {
            1 => BlockInfo { tint: Tint::Grass, ..cutout("tall_grass", "normal") },
            2 => BlockInfo { tint: Tint::Grass, ..cutout("fern", "normal") },
            _ => cutout("dead_bush", "normal"),
        },
        32 => cutout("dead_bush", "normal"),
        35 => solid(&format!("{}_wool", COLORS.get(m)?)),
        37 => cutout("dandelion", "normal"),
        38 => cutout(
            ["poppy", "blue_orchid", "allium", "houstonia", "red_tulip", "orange_tulip", "white_tulip", "pink_tulip", "oxeye_daisy"]
                .get(m)?,
            "normal",
        ),
        39 => emitting(cutout("brown_mushroom", "normal"), 1),
        40 => cutout("red_mushroom", "normal"),
        41 => solid("gold_block"),
        42 => solid("iron_block"),
        45 => solid("brick_block"),
        47 => solid("bookshelf"),
        48 => solid("mossy_cobblestone"),
        49 => solid("obsidian"),
        56 => solid("diamond_ore"),
        57 => solid("diamond_block"),
        58 => solid("crafting_table"),
        73 => solid("redstone_ore"),
        74 => emitting(solid("redstone_ore"), 9),
        79 => BlockInfo {
            layer: Layer::Translucent,
            opaque: false,
            props: LightProps { opacity: 3, ..LightProps::SOLID },
            ..solid("ice")
        },
        80 => solid("snow"),
        82 => solid("clay"),
        83 => cutout("reeds", "normal"),
        86 => BlockInfo { variant: "facing=south".into(), ..solid("pumpkin") },
        87 => solid("netherrack"),
        88 => solid("soul_sand"),
        89 => emitting(solid("glowstone"), 15),
        98 => solid(["stonebrick", "mossy_stonebrick", "cracked_stonebrick", "chiseled_stonebrick"].get(m)?),
        103 => solid("melon_block"),
        110 => BlockInfo { variant: "snowy=false".into(), ..solid("mycelium") },
        112 => solid("nether_brick"),
        121 => solid("end_stone"),
        129 => solid("emerald_ore"),
        133 => solid("emerald_block"),
        152 => solid("redstone_block"),
        153 => solid("quartz_ore"),
        155 => solid("quartz_block"),
        159 => solid(&format!("{}_stained_hardened_clay", COLORS.get(m)?)),
        161 => leaves(WOODS[4 + (m & 1)], Tint::Foliage),
        162 => BlockInfo { variant: log_axis(meta).into(), ..solid(&format!("{}_log", WOODS[4 + (m & 1)])) },
        168 => solid(["prismarine", "prismarine_bricks", "dark_prismarine"].get(m)?),
        169 => emitting(solid("sea_lantern"), 15),
        170 => BlockInfo { variant: "axis=y".into(), ..solid("hay_block") },
        172 => solid("hardened_clay"),
        173 => solid("coal_block"),
        174 => solid("packed_ice"),
        _ => return None,
    })
}

/// Every id/meta pair the table knows, for preloading models.
pub fn all_known() -> impl Iterator<Item = (u16, u8, BlockInfo)> {
    (1u16..256).flat_map(|id| (0u8..16).filter_map(move |m| lookup(id, m).map(|b| (id, m, b))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_blocks() {
        assert_eq!(lookup(1, 0).unwrap().blockstate, "stone");
        assert_eq!(lookup(17, 4 | 2).unwrap().variant, "axis=x");
        assert_eq!(lookup(17, 4 | 2).unwrap().blockstate, "birch_log");
        assert!(lookup(1, 9).is_none());
        assert!(!lookup(18, 0).unwrap().opaque);
    }
}
