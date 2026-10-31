//! Blocks and block states, mirroring `Block`, `BlockState` and `IBlockState`.
//!
//! Every state of every block has a dense [`StateId`], laid out like vanilla's
//! `BlockState.getValidStates()`: blocks in id order, and within a block the cartesian product of
//! its properties (sorted by name) with the last property varying fastest.

use crate::generated::blocks::{BLOCKS, RAW_TO_STATE, STATE_BLOCK};
use crate::material::Material;
use mc_core::Facing;
use std::collections::HashMap;
use std::fmt;
use std::sync::OnceLock;

pub use crate::generated::blocks::{ids, STATE_COUNT};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyKind {
    Bool,
    Int,
    Direction,
    Enum,
}

/// One `IProperty` of a block. `values` are the serialized names in `getAllowedValues()` order.
#[derive(Debug)]
pub struct Property {
    pub name: &'static str,
    pub kind: PropertyKind,
    pub values: &'static [&'static str],
    /// Distance between consecutive values in the block's local state index.
    pub stride: u16,
}

impl Property {
    pub fn value_index(&self, value: &str) -> Option<usize> {
        self.values.iter().position(|v| *v == value)
    }
}

/// `EnumWorldBlockLayer`
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RenderLayer {
    Solid,
    CutoutMipped,
    Cutout,
    Translucent,
}

/// `Block.EnumOffsetType`: whether a baked model is jittered within its block cell, so that a
/// field of grass or flowers doesn't look grid-aligned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OffsetType {
    None,
    /// `XZ`: horizontal jitter only.
    Xz,
    /// `XYZ`: horizontal jitter plus a downward shift.
    Xyz,
}

/// `Block.SoundType`
#[derive(Debug, PartialEq)]
pub struct SoundType {
    pub name: &'static str,
    pub volume: f32,
    pub frequency: f32,
    pub break_sound: &'static str,
    pub step_sound: &'static str,
    pub place_sound: &'static str,
}

/// Registry fields consumed by lighting and ambient occlusion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LightProps {
    pub full_cube: bool,
    pub normal_cube: bool,
    pub translucent: bool,
    pub opacity: u8,
    pub emission: u8,
}

impl LightProps {
    pub fn ao_value(self) -> f32 { if self.normal_cube { 0.2 } else { 1.0 } }
}

/// Static description of a block: what vanilla's `Block` holds after `Block.registerBlocks`.
#[derive(Debug)]
pub struct Block {
    pub id: u16,
    /// Registry name with namespace, e.g. `minecraft:stone`.
    pub name: &'static str,
    /// Vanilla class name, e.g. `BlockStone`; behaviour is dispatched on this later.
    pub class: &'static str,
    pub unlocalized_name: &'static str,
    pub material: Material,
    pub properties: &'static [Property],
    pub first_state: u16,
    pub state_count: u16,
    pub default_state: StateId,
    /// `getMetaFromState` for each local state.
    pub meta_of_state: &'static [u8],
    /// `getStateFromMeta`; `None` where vanilla throws (e.g. `leaves2` meta 2).
    pub state_of_meta: [Option<StateId>; 16],
    /// `blockHardness`; -1 is unbreakable.
    pub hardness: f32,
    /// `blockResistance` as stored (3× what `setResistance` was given).
    pub resistance: f32,
    pub slipperiness: f32,
    pub light_opacity: u8,
    pub light_value: u8,
    /// `isOpaqueCube` with fast graphics; see [`Block::is_opaque_cube`].
    pub opaque_cube: bool,
    pub opaque_cube_fancy: bool,
    pub layer: RenderLayer,
    pub layer_fancy: RenderLayer,
    pub full_cube: bool,
    pub full_block: bool,
    pub normal_cube: bool,
    pub block_normal_cube: bool,
    pub translucent: bool,
    pub use_neighbor_brightness: bool,
    pub ticks_randomly: bool,
    pub collidable: bool,
    pub enable_stats: bool,
    pub has_tile_entity: bool,
    pub can_provide_power: bool,
    pub comparator_override: bool,
    pub requires_updates: bool,
    pub can_spawn_in: bool,
    /// `getMobilityFlag`
    pub mobility: u8,
    /// `getRenderType`: -1 invisible, 1 liquid, 2 tile-entity special renderer, 3 baked model.
    pub render_type: i8,
    pub ao_light_value: f32,
    pub sound: &'static SoundType,
    /// Bounds right after construction (`minX, minY, minZ, maxX, maxY, maxZ`); state-dependent
    /// bounds come from `setBlockBoundsBasedOnState`.
    pub default_bounds: [f64; 6],
    /// Class and its superclasses up to `Block`, for vanilla's `instanceof` checks.
    pub class_chain: &'static [&'static str],
    /// Whether the class overrides `getActualState` (see [`crate::actual_state`]).
    pub actual_state_override: bool,
    /// `BlockFire.setFireInfo` values; 0 when the block isn't registered there.
    pub fire_encouragement: u8,
    pub fire_flammability: u8,
    /// `BlockRedstoneDiode.isRepeaterPowered` (the `powered_*` repeater/comparator blocks).
    pub diode_powered: bool,
    /// `BlockStem.crop`: the fruit block a stem points at.
    pub stem_crop: Option<u16>,
}

impl Block {
    pub fn all() -> &'static [Block] {
        &BLOCKS
    }

    pub fn by_id(id: u16) -> Option<&'static Block> {
        BLOCKS.get(id as usize)
    }

    /// `Block.getBlockFromName`: accepts `stone` or `minecraft:stone`.
    pub fn by_name(name: &str) -> Option<&'static Block> {
        static MAP: OnceLock<HashMap<&'static str, u16>> = OnceLock::new();
        let map = MAP.get_or_init(|| BLOCKS.iter().map(|b| (b.name, b.id)).collect());
        let key = if name.contains(':') { name.to_string() } else { format!("minecraft:{name}") };
        map.get(key.as_str()).map(|&id| &BLOCKS[id as usize])
    }

    pub fn states(&self) -> impl Iterator<Item = StateId> {
        (self.first_state..self.first_state + self.state_count).map(StateId)
    }

    pub fn state_from_meta(&self, meta: u8) -> Option<StateId> {
        self.state_of_meta.get(meta as usize).copied().flatten()
    }

    pub fn property(&self, name: &str) -> Option<(usize, &'static Property)> {
        self.properties.iter().enumerate().find(|(_, p)| p.name == name)
    }

    /// `isOpaqueCube`; leaves follow the graphics setting (`BlockLeaves.setGraphicsLevel`).
    pub fn is_opaque_cube(&self, fancy_graphics: bool) -> bool {
        if fancy_graphics { self.opaque_cube_fancy } else { self.opaque_cube }
    }

    /// `getBlockLayer`, with the same graphics dependence as [`Block::is_opaque_cube`].
    pub fn render_layer(&self, fancy_graphics: bool) -> RenderLayer {
        if fancy_graphics { self.layer_fancy } else { self.layer }
    }

    /// `getExplosionResistance(Entity)`
    pub fn explosion_resistance(&self) -> f32 {
        self.resistance / 5.0
    }

    /// `getOffsetType`: only `BlockTallGrass` (XYZ), `BlockFlower` (XZ) and `BlockDoublePlant`
    /// (XZ) override it in 1.8.9. Everything else keeps NONE, including `BlockDeadBush` and
    /// `BlockReed` as well as saplings, crops, stems, mushrooms and `BlockLilyPad`.
    pub fn offset_type(&self) -> OffsetType {
        if self.is_a("BlockTallGrass") {
            OffsetType::Xyz
        } else if self.is_a("BlockFlower") || self.is_a("BlockDoublePlant") {
            OffsetType::Xz
        } else {
            OffsetType::None
        }
    }

    /// `this instanceof <class>` for a vanilla class name such as `BlockSlab`.
    pub fn is_a(&self, class: &str) -> bool {
        self.class_chain.contains(&class)
    }
}

/// A block state (`IBlockState`). Copyable, comparable, hashable; `0` is air.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StateId(pub u16);

impl StateId {
    pub const AIR: StateId = StateId(0);

    #[inline]
    pub fn block(self) -> &'static Block {
        &BLOCKS[STATE_BLOCK[self.0 as usize] as usize]
    }

    #[inline]
    pub fn is_air(self) -> bool {
        self.0 == 0
    }

    pub fn light_props(self) -> LightProps {
        let b = self.block();
        LightProps {
            full_cube: b.full_cube, normal_cube: b.block_normal_cube,
            translucent: b.translucent, opacity: b.light_opacity, emission: b.light_value,
        }
    }

    /// Index within the block's own states.
    #[inline]
    pub fn local(self) -> u16 {
        self.0 - self.block().first_state
    }

    /// `getMetaFromState`
    #[inline]
    pub fn meta(self) -> u8 {
        self.block().meta_of_state[self.local() as usize]
    }

    /// Chunk storage value, `id << 4 | meta` (`Block.BLOCK_STATE_IDS`).
    #[inline]
    pub fn raw(self) -> u16 {
        self.block().id << 4 | self.meta() as u16
    }

    /// `Block.getStateId`: `id + (meta << 12)`, used by packets and particles.
    #[inline]
    pub fn network_id(self) -> i32 {
        self.block().id as i32 + ((self.meta() as i32) << 12)
    }

    /// What chunk storage reads for a raw `id << 4 | meta` value
    /// (`ExtendedBlockStorage.get`): unregistered values are air.
    #[inline]
    pub fn from_raw(raw: u16) -> StateId {
        Self::try_from_raw(raw).unwrap_or(StateId::AIR)
    }

    /// `Block.getStateById`: `id + (meta << 12)` from packets.
    pub fn try_from_network(v: i32) -> Option<StateId> {
        let (id, meta) = (v & 4095, (v >> 12) & 15);
        Self::try_from_raw(((id as u32) << 4 | meta as u32) as u16)
    }

    #[inline]
    pub fn try_from_raw(raw: u16) -> Option<StateId> {
        match RAW_TO_STATE.get(raw as usize) {
            Some(&s) if s != u16::MAX => Some(StateId(s)),
            _ => None,
        }
    }

    /// Value index of the block's `prop`-th property.
    #[inline]
    pub fn value_index(self, prop: usize) -> usize {
        let p = &self.block().properties[prop];
        (self.local() / p.stride) as usize % p.values.len()
    }

    /// Serialized value of the property called `name`, if the block has it.
    pub fn value(self, name: &str) -> Option<&'static str> {
        let (i, p) = self.block().property(name)?;
        Some(p.values[self.value_index(i)])
    }

    /// `withProperty` by indices. Panics if either index is out of range.
    pub fn with_index(self, prop: usize, value: usize) -> StateId {
        let b = self.block();
        let p = &b.properties[prop];
        assert!(value < p.values.len(), "value {value} out of range for {}", p.name);
        let cur = self.value_index(prop);
        StateId((self.0 as i32 + (value as i32 - cur as i32) * p.stride as i32) as u16)
    }

    /// `withProperty` by names; `None` if the block lacks the property or value
    /// (vanilla throws `IllegalArgumentException`).
    pub fn with(self, name: &str, value: &str) -> Option<StateId> {
        let (i, p) = self.block().property(name)?;
        Some(self.with_index(i, p.value_index(value)?))
    }

    /// `cycleProperty`: next allowed value, wrapping around.
    pub fn cycle(self, name: &str) -> Option<StateId> {
        let (i, p) = self.block().property(name)?;
        Some(self.with_index(i, (self.value_index(i) + 1) % p.values.len()))
    }

    /// `getValue` of a boolean property. Panics if the block lacks it (vanilla throws).
    pub fn get_bool(self, name: &str) -> bool {
        self.expect(name) == "true"
    }

    /// `getValue` of a direction property.
    pub fn get_facing(self, name: &str) -> Facing {
        Facing::by_name(self.expect(name)).unwrap_or_else(|| panic!("{self}: {name} is not a direction"))
    }

    /// `getValue` of an integer property.
    pub fn get_int(self, name: &str) -> i32 {
        self.expect(name).parse().unwrap_or_else(|_| panic!("{self}: {name} is not an integer"))
    }

    /// `getValue` by name. Panics if the block lacks the property (vanilla throws).
    pub fn expect(self, name: &str) -> &'static str {
        self.value(name).unwrap_or_else(|| panic!("Cannot get property {name} as it does not exist in {}", self.block().name))
    }

    /// `withProperty` that panics like vanilla when the property or value is missing.
    pub fn set(self, name: &str, value: &str) -> StateId {
        self.with(name, value).unwrap_or_else(|| panic!("Cannot set property {name}={value} on {self}"))
    }

    pub fn set_bool(self, name: &str, value: bool) -> StateId {
        self.set(name, if value { "true" } else { "false" })
    }

    /// `(name, value)` pairs in property order (sorted by name).
    pub fn properties(self) -> impl Iterator<Item = (&'static str, &'static str)> {
        let b = self.block();
        b.properties.iter().enumerate().map(move |(i, p)| (p.name, p.values[self.value_index(i)]))
    }
}

/// `BlockStateBase.toString`: `minecraft:stone[variant=granite]`.
impl fmt::Display for StateId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.block().name)?;
        let mut props = self.properties().peekable();
        if props.peek().is_some() {
            f.write_str("[")?;
            for (i, (k, v)) in props.enumerate() {
                if i > 0 {
                    f.write_str(",")?;
                }
                write!(f, "{k}={v}")?;
            }
            f.write_str("]")?;
        }
        Ok(())
    }
}

impl fmt::Debug for StateId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "StateId({}: {self})", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn air_is_zero() {
        assert_eq!(StateId::AIR.block().name, "minecraft:air");
        assert_eq!(Block::by_id(0).unwrap().default_state, StateId::AIR);
        assert_eq!(StateId::from_raw(0), StateId::AIR);
    }

    #[test]
    fn property_round_trip() {
        let door = Block::by_name("wooden_door").unwrap();
        for s in door.states() {
            for (i, p) in door.properties.iter().enumerate() {
                for v in 0..p.values.len() {
                    let t = s.with_index(i, v);
                    assert_eq!(t.block().id, door.id);
                    assert_eq!(t.value_index(i), v);
                    for j in (0..door.properties.len()).filter(|&j| j != i) {
                        assert_eq!(t.value_index(j), s.value_index(j));
                    }
                }
            }
        }
    }

    #[test]
    fn display_matches_vanilla_format() {
        let s = Block::by_name("stone").unwrap().state_from_meta(1).unwrap();
        assert_eq!(s.to_string(), "minecraft:stone[variant=granite]");
        assert_eq!(Block::by_name("minecraft:air").unwrap().default_state.to_string(), "minecraft:air");
    }

    #[test]
    fn cycle_wraps() {
        let rep = Block::by_name("unpowered_repeater").unwrap().default_state;
        assert_eq!(rep.value("delay"), Some("1"));
        let mut s = rep;
        for _ in 0..4 {
            s = s.cycle("delay").unwrap();
        }
        assert_eq!(s, rep);
    }

    /// `Block.getOffsetType` overrides in 1.8.9: `BlockTallGrass` returns XYZ, `BlockFlower` (both
    /// concrete subclasses) and `BlockDoublePlant` return XZ, and every other block inherits NONE.
    #[test]
    fn offset_type_matches_vanilla_overrides() {
        assert_eq!(Block::by_name("tallgrass").unwrap().offset_type(), OffsetType::Xyz);
        for name in ["red_flower", "yellow_flower", "double_plant"] {
            assert_eq!(Block::by_name(name).unwrap().offset_type(), OffsetType::Xz, "{name}");
        }
        // `BlockDeadBush` and `BlockReed` extend `BlockBush`/`Block` without overriding it, so
        // they are NONE, as are saplings, crops, stems, mushrooms and lily pads in 1.8.9.
        for name in [
            "deadbush", "reeds", "sapling", "wheat", "carrots", "potatoes", "pumpkin_stem",
            "melon_stem", "brown_mushroom", "red_mushroom", "nether_wart", "waterlily", "stone", "air",
        ] {
            assert_eq!(Block::by_name(name).unwrap().offset_type(), OffsetType::None, "{name}");
        }
    }

    /// The offset type is per block class, not per state: the tall-grass `dead_bush` variant and
    /// both halves of a double plant keep their class's offset (unlike the separate dead-bush block).
    #[test]
    fn offset_type_does_not_depend_on_state() {
        let tall = Block::by_name("tallgrass").unwrap();
        assert_eq!(tall.state_from_meta(0).unwrap(), tall.default_state);
        assert_eq!(tall.default_state.block().offset_type(), OffsetType::Xyz);
        assert_eq!(tall.state_from_meta(2).unwrap().block().offset_type(), OffsetType::Xyz);
        let plant = Block::by_name("double_plant").unwrap();
        for s in plant.states() {
            assert_eq!(s.block().offset_type(), OffsetType::Xz);
        }
    }
}
