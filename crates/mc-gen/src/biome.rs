//! Worldgen view of `BiomeGenBase` and its subclasses: what each biome id is, built the way the
//! static initializer builds it (including `createMutation`).

use std::sync::OnceLock;

/// Concrete Java class (or behaviour variant) of a biome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Ocean,
    /// `BiomeGenPlains`; `true` for Sunflower Plains.
    Plains(bool),
    Desert,
    /// `BiomeGenHills` with `field_150638_aH` (0 normal, 1 "plus/edge", 2 mutated).
    Hills(u8),
    /// `BiomeGenForest` type: 0 forest, 1 flower, 2 birch, 3 roofed.
    Forest(u8),
    /// `BiomeGenTaiga` type: 0 taiga, 1 mega, 2 mega spruce.
    Taiga(u8),
    Swamp,
    River,
    Hell,
    End,
    /// `BiomeGenSnow`; `true` for Ice Plains Spikes.
    Snow(bool),
    Mushroom,
    Beach,
    StoneBeach,
    /// `BiomeGenJungle`; `true` for the edge variant.
    Jungle(bool),
    Savanna,
    /// `BiomeGenSavanna.Mutated`
    SavannaMutated,
    /// `BiomeGenMesa(bryce, forest)`
    Mesa(bool, bool),
    /// `BiomeGenMutated` around a base id. `MutatedForest` variants keep vanilla's anonymous
    /// subclasses: roofed forest M decorates through its base, birch M picks tall birches.
    Mutated,
}

/// The class identity used by `getBiomeClass` / `isEqualTo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Ocean,
    Plains,
    Desert,
    Hills,
    Forest,
    Taiga,
    Swamp,
    River,
    Hell,
    End,
    Snow,
    Mushroom,
    Beach,
    StoneBeach,
    Jungle,
    Savanna,
    Mesa,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TempCategory {
    Ocean,
    Cold,
    Medium,
    Warm,
}

/// `BiomeDecorator` per-chunk counts.
#[derive(Clone, Copy, Debug)]
pub struct Decorator {
    pub waterlily: i32,
    pub trees: i32,
    pub flowers: i32,
    pub grass: i32,
    pub dead_bush: i32,
    pub mushrooms: i32,
    pub reeds: i32,
    pub cacti: i32,
    pub sand: i32,
    pub sand2: i32,
    pub clay: i32,
    pub big_mushrooms: i32,
    pub generate_lakes: bool,
}

impl Default for Decorator {
    fn default() -> Self {
        Self {
            waterlily: 0,
            trees: 0,
            flowers: 2,
            grass: 1,
            dead_bush: 0,
            mushrooms: 0,
            reeds: 0,
            cacti: 0,
            sand: 1,
            sand2: 3,
            clay: 1,
            big_mushrooms: 0,
            generate_lakes: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct GenBiome {
    pub id: u8,
    pub name: &'static str,
    pub kind: Kind,
    /// `baseBiome` for mutated biomes, else own id.
    pub base: u8,
    pub min_height: f32,
    pub max_height: f32,
    pub temperature: f32,
    pub rainfall: f32,
    pub enable_snow: bool,
    pub enable_rain: bool,
    /// `topBlock` / `fillerBlock` as `(id, meta)`.
    pub top: (u16, u8),
    pub filler: (u16, u8),
    pub decorator: Decorator,
}

const H_DEFAULT: (f32, f32) = (0.1, 0.2);
const H_SHALLOW_WATERS: (f32, f32) = (-0.5, 0.0);
const H_OCEANS: (f32, f32) = (-1.0, 0.1);
const H_DEEP_OCEANS: (f32, f32) = (-1.8, 0.1);
const H_LOW_PLAINS: (f32, f32) = (0.125, 0.05);
const H_MID_PLAINS: (f32, f32) = (0.2, 0.2);
const H_LOW_HILLS: (f32, f32) = (0.45, 0.3);
const H_HIGH_PLATEAUS: (f32, f32) = (1.5, 0.025);
const H_MID_HILLS: (f32, f32) = (1.0, 0.5);
const H_SHORES: (f32, f32) = (0.0, 0.025);
const H_ROCKY_WATERS: (f32, f32) = (0.1, 0.8);
const H_LOW_ISLANDS: (f32, f32) = (0.2, 0.3);
const H_PARTIALLY_SUBMERGED: (f32, f32) = (-0.2, 0.1);

fn attenuate(h: (f32, f32)) -> (f32, f32) {
    (h.0 * 0.8, h.1 * 0.6)
}

impl GenBiome {
    fn new(id: u8, name: &'static str, kind: Kind) -> Self {
        let mut b = Self {
            id,
            name,
            kind,
            base: id,
            min_height: H_DEFAULT.0,
            max_height: H_DEFAULT.1,
            temperature: 0.5,
            rainfall: 0.5,
            enable_snow: false,
            enable_rain: true,
            top: (2, 0),
            filler: (3, 0),
            decorator: Decorator::default(),
        };
        let d = &mut b.decorator;
        match kind {
            Kind::Plains(_) => {
                b.temperature = 0.8;
                b.rainfall = 0.4;
                (b.min_height, b.max_height) = H_LOW_PLAINS;
                d.trees = -999;
                d.flowers = 4;
                d.grass = 10;
            }
            Kind::Desert => {
                b.top = (12, 0);
                b.filler = (12, 0);
                d.trees = -999;
                d.dead_bush = 2;
                d.reeds = 50;
                d.cacti = 10;
            }
            Kind::Hills(m) => {
                if m == 1 {
                    d.trees = 3;
                }
            }
            Kind::Forest(t) => {
                d.trees = 10;
                d.grass = 2;
                if t == 1 {
                    d.trees = 6;
                    d.flowers = 100;
                    d.grass = 1;
                }
                b.temperature = 0.7;
                b.rainfall = 0.8;
                if t == 2 {
                    b.temperature = 0.6;
                    b.rainfall = 0.6;
                }
                if t == 3 {
                    d.trees = -999;
                }
            }
            Kind::Taiga(t) => {
                d.trees = 10;
                if t != 1 && t != 2 {
                    d.grass = 1;
                    d.mushrooms = 1;
                } else {
                    d.grass = 7;
                    d.dead_bush = 1;
                    d.mushrooms = 3;
                }
            }
            Kind::Swamp => {
                d.trees = 2;
                d.flowers = 1;
                d.dead_bush = 1;
                d.mushrooms = 8;
                d.reeds = 10;
                d.clay = 1;
                d.waterlily = 4;
                d.sand2 = 0;
                d.sand = 0;
                d.grass = 5;
            }
            Kind::Snow(spikes) => {
                if spikes {
                    b.top = (80, 0);
                }
            }
            Kind::Mushroom => {
                d.trees = -100;
                d.flowers = -100;
                d.grass = -100;
                d.mushrooms = 1;
                d.big_mushrooms = 1;
                b.top = (110, 0);
            }
            Kind::Beach | Kind::StoneBeach => {
                if kind == Kind::Beach {
                    b.top = (12, 0);
                    b.filler = (12, 0);
                } else {
                    b.top = (1, 0);
                    b.filler = (1, 0);
                }
                d.trees = -999;
                d.dead_bush = 0;
                d.reeds = 0;
                d.cacti = 0;
            }
            Kind::Jungle(edge) => {
                d.trees = if edge { 2 } else { 50 };
                d.grass = 25;
                d.flowers = 4;
            }
            Kind::Savanna => {
                d.trees = 1;
                d.flowers = 4;
                d.grass = 20;
            }
            Kind::Mesa(_, forest) => {
                b.enable_rain = false;
                b.temperature = 2.0;
                b.rainfall = 0.0;
                b.top = (12, 1);
                b.filler = (159, 0);
                d.trees = -999;
                d.dead_bush = 20;
                d.reeds = 3;
                d.cacti = 5;
                d.flowers = 0;
                if forest {
                    d.trees = 5;
                }
            }
            Kind::Hell | Kind::End => {}
            _ => {}
        }
        b
    }

    fn height(mut self, h: (f32, f32)) -> Self {
        (self.min_height, self.max_height) = h;
        self
    }

    fn temp(mut self, t: f32, r: f32) -> Self {
        self.temperature = t;
        self.rainfall = r;
        self
    }

    fn snow(mut self) -> Self {
        self.enable_snow = true;
        self
    }

    fn no_rain(mut self) -> Self {
        self.enable_rain = false;
        self
    }

    /// `new BiomeGenMutated(id, base)`
    fn mutated(id: u8, name: &'static str, base: &GenBiome) -> Self {
        let mut b = Self::new(id, name, Kind::Mutated);
        b.base = base.id;
        b.top = base.top;
        b.filler = base.filler;
        b.temperature = base.temperature;
        b.rainfall = base.rainfall;
        b.enable_snow = base.enable_snow;
        b.enable_rain = base.enable_rain;
        b.min_height = base.min_height + 0.1;
        b.max_height = base.max_height + 0.2;
        b
    }

    /// `getBiomeClass`
    pub fn class(&self) -> Class {
        match self.kind {
            Kind::Ocean => Class::Ocean,
            Kind::Plains(_) => Class::Plains,
            Kind::Desert => Class::Desert,
            Kind::Hills(_) => Class::Hills,
            Kind::Forest(_) => Class::Forest,
            Kind::Taiga(_) => Class::Taiga,
            Kind::Swamp => Class::Swamp,
            Kind::River => Class::River,
            Kind::Hell => Class::Hell,
            Kind::End => Class::End,
            Kind::Snow(_) => Class::Snow,
            Kind::Mushroom => Class::Mushroom,
            Kind::Beach => Class::Beach,
            Kind::StoneBeach => Class::StoneBeach,
            Kind::Jungle(_) => Class::Jungle,
            Kind::Savanna | Kind::SavannaMutated => Class::Savanna,
            Kind::Mesa(..) => Class::Mesa,
            Kind::Mutated => biome(self.base as i32).unwrap().class(),
        }
    }

    /// `getTempCategory`
    pub fn temp_category(&self) -> TempCategory {
        match self.kind {
            Kind::Ocean => TempCategory::Ocean,
            Kind::Mutated | Kind::SavannaMutated => biome(self.base as i32).unwrap().temp_category(),
            _ => {
                let t = self.temperature as f64;
                if t < 0.2 {
                    TempCategory::Cold
                } else if t < 1.0 {
                    TempCategory::Medium
                } else {
                    TempCategory::Warm
                }
            }
        }
    }

    /// `isEqualTo`
    pub fn is_equal_to(&self, other: &GenBiome) -> bool {
        other.id == self.id || self.class() == other.class()
    }

    pub fn is_snowy(&self) -> bool {
        self.enable_snow
    }

    pub fn base_biome(&self) -> &'static GenBiome {
        biome(self.base as i32).unwrap()
    }
}

/// `BiomeGenBase.getBiome(id)`: `None` for unregistered ids.
pub fn biome(id: i32) -> Option<&'static GenBiome> {
    if !(0..256).contains(&id) {
        return None;
    }
    table()[id as usize].as_ref()
}

/// `getBiomeFromBiomeList(id, ocean)`
pub fn biome_or_ocean(id: i32) -> &'static GenBiome {
    biome(id).unwrap_or_else(|| biome(0).unwrap())
}

pub fn table() -> &'static [Option<GenBiome>; 256] {
    static T: OnceLock<[Option<GenBiome>; 256]> = OnceLock::new();
    T.get_or_init(build)
}

pub mod ids {
    pub const OCEAN: i32 = 0;
    pub const PLAINS: i32 = 1;
    pub const DESERT: i32 = 2;
    pub const EXTREME_HILLS: i32 = 3;
    pub const FOREST: i32 = 4;
    pub const TAIGA: i32 = 5;
    pub const SWAMPLAND: i32 = 6;
    pub const RIVER: i32 = 7;
    pub const HELL: i32 = 8;
    pub const SKY: i32 = 9;
    pub const FROZEN_OCEAN: i32 = 10;
    pub const FROZEN_RIVER: i32 = 11;
    pub const ICE_PLAINS: i32 = 12;
    pub const ICE_MOUNTAINS: i32 = 13;
    pub const MUSHROOM_ISLAND: i32 = 14;
    pub const MUSHROOM_ISLAND_SHORE: i32 = 15;
    pub const BEACH: i32 = 16;
    pub const DESERT_HILLS: i32 = 17;
    pub const FOREST_HILLS: i32 = 18;
    pub const TAIGA_HILLS: i32 = 19;
    pub const EXTREME_HILLS_EDGE: i32 = 20;
    pub const JUNGLE: i32 = 21;
    pub const JUNGLE_HILLS: i32 = 22;
    pub const JUNGLE_EDGE: i32 = 23;
    pub const DEEP_OCEAN: i32 = 24;
    pub const STONE_BEACH: i32 = 25;
    pub const COLD_BEACH: i32 = 26;
    pub const BIRCH_FOREST: i32 = 27;
    pub const BIRCH_FOREST_HILLS: i32 = 28;
    pub const ROOFED_FOREST: i32 = 29;
    pub const COLD_TAIGA: i32 = 30;
    pub const COLD_TAIGA_HILLS: i32 = 31;
    pub const MEGA_TAIGA: i32 = 32;
    pub const MEGA_TAIGA_HILLS: i32 = 33;
    pub const EXTREME_HILLS_PLUS: i32 = 34;
    pub const SAVANNA: i32 = 35;
    pub const SAVANNA_PLATEAU: i32 = 36;
    pub const MESA: i32 = 37;
    pub const MESA_PLATEAU_F: i32 = 38;
    pub const MESA_PLATEAU: i32 = 39;
}

fn build() -> [Option<GenBiome>; 256] {
    use Kind::*;
    let mut t: [Option<GenBiome>; 256] = std::array::from_fn(|_| None);
    let mut put = |b: GenBiome| {
        let id = b.id as usize;
        t[id] = Some(b);
    };
    put(GenBiome::new(0, "Ocean", Ocean).height(H_OCEANS));
    put(GenBiome::new(1, "Plains", Plains(false)));
    put(GenBiome::new(2, "Desert", Desert).no_rain().temp(2.0, 0.0).height(H_LOW_PLAINS));
    put(GenBiome::new(3, "Extreme Hills", Hills(0)).height(H_MID_HILLS).temp(0.2, 0.3));
    put(GenBiome::new(4, "Forest", Forest(0)));
    put(GenBiome::new(5, "Taiga", Taiga(0)).temp(0.25, 0.8).height(H_MID_PLAINS));
    put(GenBiome::new(6, "Swampland", Swamp).height(H_PARTIALLY_SUBMERGED).temp(0.8, 0.9));
    put(GenBiome::new(7, "River", River).height(H_SHALLOW_WATERS));
    put(GenBiome::new(8, "Hell", Hell).no_rain().temp(2.0, 0.0));
    put(GenBiome::new(9, "The End", End).no_rain());
    put(GenBiome::new(10, "FrozenOcean", Ocean).snow().height(H_OCEANS).temp(0.0, 0.5));
    put(GenBiome::new(11, "FrozenRiver", River).snow().height(H_SHALLOW_WATERS).temp(0.0, 0.5));
    put(GenBiome::new(12, "Ice Plains", Snow(false)).snow().temp(0.0, 0.5).height(H_LOW_PLAINS));
    put(GenBiome::new(13, "Ice Mountains", Snow(false)).snow().height(H_LOW_HILLS).temp(0.0, 0.5));
    put(GenBiome::new(14, "MushroomIsland", Mushroom).temp(0.9, 1.0).height(H_LOW_ISLANDS));
    put(GenBiome::new(15, "MushroomIslandShore", Mushroom).temp(0.9, 1.0).height(H_SHORES));
    put(GenBiome::new(16, "Beach", Beach).temp(0.8, 0.4).height(H_SHORES));
    put(GenBiome::new(17, "DesertHills", Desert).no_rain().temp(2.0, 0.0).height(H_LOW_HILLS));
    put(GenBiome::new(18, "ForestHills", Forest(0)).height(H_LOW_HILLS));
    put(GenBiome::new(19, "TaigaHills", Taiga(0)).temp(0.25, 0.8).height(H_LOW_HILLS));
    put(GenBiome::new(20, "Extreme Hills Edge", Hills(1)).height(attenuate(H_MID_HILLS)).temp(0.2, 0.3));
    put(GenBiome::new(21, "Jungle", Jungle(false)).temp(0.95, 0.9));
    put(GenBiome::new(22, "JungleHills", Jungle(false)).temp(0.95, 0.9).height(H_LOW_HILLS));
    put(GenBiome::new(23, "JungleEdge", Jungle(true)).temp(0.95, 0.8));
    put(GenBiome::new(24, "Deep Ocean", Ocean).height(H_DEEP_OCEANS));
    put(GenBiome::new(25, "Stone Beach", StoneBeach).temp(0.2, 0.3).height(H_ROCKY_WATERS));
    put(GenBiome::new(26, "Cold Beach", Beach).temp(0.05, 0.3).height(H_SHORES).snow());
    put(GenBiome::new(27, "Birch Forest", Forest(2)));
    put(GenBiome::new(28, "Birch Forest Hills", Forest(2)).height(H_LOW_HILLS));
    put(GenBiome::new(29, "Roofed Forest", Forest(3)));
    put(GenBiome::new(30, "Cold Taiga", Taiga(0)).snow().temp(-0.5, 0.4).height(H_MID_PLAINS));
    put(GenBiome::new(31, "Cold Taiga Hills", Taiga(0)).snow().temp(-0.5, 0.4).height(H_LOW_HILLS));
    put(GenBiome::new(32, "Mega Taiga", Taiga(1)).temp(0.3, 0.8).height(H_MID_PLAINS));
    put(GenBiome::new(33, "Mega Taiga Hills", Taiga(1)).temp(0.3, 0.8).height(H_LOW_HILLS));
    put(GenBiome::new(34, "Extreme Hills+", Hills(1)).height(H_MID_HILLS).temp(0.2, 0.3));
    put(GenBiome::new(35, "Savanna", Savanna).temp(1.2, 0.0).no_rain().height(H_LOW_PLAINS));
    put(GenBiome::new(36, "Savanna Plateau", Savanna).temp(1.0, 0.0).no_rain().height(H_HIGH_PLATEAUS));
    put(GenBiome::new(37, "Mesa", Mesa(false, false)));
    put(GenBiome::new(38, "Mesa Plateau F", Mesa(false, true)).height(H_HIGH_PLATEAUS));
    put(GenBiome::new(39, "Mesa Plateau", Mesa(false, false)).height(H_HIGH_PLATEAUS));

    // Mutations, in the static initializer's order.
    let g = |t: &[Option<GenBiome>; 256], id: usize| t[id].clone().unwrap();
    let plains = g(&t, 1);
    t[129] = Some(GenBiome::new(129, "Sunflower Plains", Plains(true)));
    let _ = plains;
    let m = GenBiome::mutated(130, "Desert M", &g(&t, 2));
    t[130] = Some(m);
    let forest = g(&t, 4);
    t[132] = Some(GenBiome::new(132, "Flower Forest", Forest(1)).height((forest.min_height, forest.max_height + 0.2)));
    t[133] = Some(GenBiome::mutated(133, "Taiga M", &g(&t, 5)));
    t[134] = Some(GenBiome::mutated(134, "Swampland M", &g(&t, 6)));
    let ice = g(&t, 12);
    let mut spikes = GenBiome::new(140, "Ice Plains Spikes", Snow(true)).snow().temp(0.0, 0.5);
    spikes.min_height = ice.min_height + 0.3;
    spikes.max_height = ice.max_height + 0.4;
    t[140] = Some(spikes);
    t[149] = Some(GenBiome::mutated(149, "Jungle M", &g(&t, 21)));
    t[151] = Some(GenBiome::mutated(151, "JungleEdge M", &g(&t, 23)));
    t[158] = Some(GenBiome::mutated(158, "Cold Taiga M", &g(&t, 30)));
    for (id, base, name) in [(163u8, 35usize, "Savanna M"), (164, 36, "Savanna Plateau M")] {
        let b = g(&t, base);
        let mut s = GenBiome::mutated(id, name, &b);
        s.kind = SavannaMutated;
        s.decorator.trees = 2;
        s.decorator.flowers = 2;
        s.decorator.grass = 5;
        s.temperature = (b.temperature + 1.0) * 0.5;
        s.min_height = b.min_height * 0.5 + 0.3;
        s.max_height = b.max_height * 0.5 + 1.2;
        t[id as usize] = Some(s);
    }
    t[165] = Some(GenBiome::new(165, "Mesa (Bryce)", Mesa(true, false)));
    t[166] = Some(GenBiome::new(166, "Mesa Plateau F M", Mesa(false, true)).height(H_LOW_HILLS));
    t[167] = Some(GenBiome::new(167, "Mesa Plateau M", Mesa(false, false)).height(H_LOW_HILLS));
    t[155] = Some(GenBiome::mutated(155, "Birch Forest M", &g(&t, 27)));
    t[156] = Some(GenBiome::mutated(156, "Birch Forest Hills M", &g(&t, 28)));
    t[157] = Some(GenBiome::mutated(157, "Roofed Forest M", &g(&t, 29)));
    let mega = g(&t, 32);
    t[160] = Some(GenBiome::new(160, "Mega Spruce Taiga", Taiga(2)).temp(0.25, 0.8).height((mega.min_height, mega.max_height)));
    for (id, base, name) in [(131u8, 3usize, "Extreme Hills M"), (162, 34, "Extreme Hills+ M")] {
        let b = g(&t, base);
        t[id as usize] = Some(GenBiome::new(id, name, Hills(2)).height((b.min_height, b.max_height)).temp(b.temperature, b.rainfall));
    }
    t[161] = Some(GenBiome::new(161, "Redwood Taiga Hills M", Taiga(2)).temp(0.25, 0.8).height((mega.min_height, mega.max_height)));
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutated_heights_follow_base() {
        assert_eq!(biome(130).unwrap().min_height, 0.125 + 0.1);
        assert_eq!(biome(163).unwrap().temperature, 1.1);
        assert_eq!(biome(149).unwrap().class(), Class::Jungle);
        assert_eq!(biome(0).unwrap().temp_category(), TempCategory::Ocean);
        assert!(biome(128).is_none());
    }
}
