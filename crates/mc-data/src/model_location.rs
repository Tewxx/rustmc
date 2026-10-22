//! `BlockModelShapes.registerAllBlocks` and the 1.8 state mappers.

use crate::{ids, StateId};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ModelLocation {
    /// Resource location, including its namespace.
    pub name: String,
    pub variant: String,
}

impl StateId {
    /// The model resource location for this state. Built-in renderers have no JSON model.
    /// Call `actual_state` first when selecting a model in the world.
    pub fn model_location(self) -> Option<ModelLocation> {
        use ids::*;
        let id = self.block().id;
        if matches!(
            id,
            AIR | FLOWING_WATER
                | WATER
                | FLOWING_LAVA
                | LAVA
                | PISTON_EXTENSION
                | CHEST
                | ENDER_CHEST
                | TRAPPED_CHEST
                | STANDING_SIGN
                | SKULL
                | END_PORTAL
                | BARRIER
                | WALL_SIGN
                | WALL_BANNER
                | STANDING_BANNER
        ) {
            return None;
        }
        let mut name = self.block().name.trim_start_matches("minecraft:").to_string();
        let (name_prop, suffix, ignored): (Option<&str>, &str, &[&str]) = match id {
            STONE | PRISMARINE | SAND | STONEBRICK => (Some("variant"), "", &[]),
            LEAVES | LEAVES2 => (Some("variant"), "_leaves", &["check_decay", "decayable"]),
            CACTUS | REEDS | FIRE => (None, "", &["age"]),
            JUKEBOX => (None, "", &["has_record"]),
            COMMAND_BLOCK | DISPENSER | DROPPER => (None, "", &["triggered"]),
            COBBLESTONE_WALL => (Some("variant"), "_wall", &[]),
            DOUBLE_PLANT => (Some("variant"), "", &["facing"]),
            FENCE_GATE | SPRUCE_FENCE_GATE | BIRCH_FENCE_GATE | JUNGLE_FENCE_GATE | DARK_OAK_FENCE_GATE | ACACIA_FENCE_GATE
            | WOODEN_DOOR | SPRUCE_DOOR | BIRCH_DOOR | JUNGLE_DOOR | ACACIA_DOOR | DARK_OAK_DOOR | IRON_DOOR => (None, "", &["powered"]),
            TRIPWIRE => (None, "", &["disarmed", "powered"]),
            DOUBLE_WOODEN_SLAB => (Some("variant"), "_double_slab", &[]),
            WOODEN_SLAB | STONE_SLAB | STONE_SLAB2 => (Some("variant"), "_slab", &[]),
            TNT => (None, "", &["explode"]),
            REDSTONE_WIRE => (None, "", &["power"]),
            WOOL => (Some("color"), "_wool", &[]),
            CARPET => (Some("color"), "_carpet", &[]),
            STAINED_HARDENED_CLAY => (Some("color"), "_stained_hardened_clay", &[]),
            STAINED_GLASS_PANE => (Some("color"), "_stained_glass_pane", &[]),
            STAINED_GLASS => (Some("color"), "_stained_glass", &[]),
            SANDSTONE | RED_SANDSTONE | TALLGRASS | YELLOW_FLOWER | RED_FLOWER => (Some("type"), "", &[]),
            BED => (None, "", &["occupied"]),
            MONSTER_EGG => (Some("variant"), "_monster_egg", &[]),
            LOG | LOG2 => (Some("variant"), "_log", &[]),
            PLANKS => (Some("variant"), "_planks", &[]),
            SAPLING => (Some("type"), "_sapling", &[]),
            HOPPER => (None, "", &["enabled"]),
            FLOWER_POT => (None, "", &["legacy_data"]),
            DIRT if self.expect("variant") != "podzol" => (Some("variant"), "", &["snowy"]),
            DIRT => (Some("variant"), "", &[]),
            PUMPKIN_STEM | MELON_STEM if self.expect("facing") != "up" => (None, "", &["age"]),
            _ => (None, "", &[]),
        };
        if let Some(prop) = name_prop {
            name = format!("{}{suffix}", self.expect(prop));
        }
        let mut variant = self
            .properties()
            .filter(|(p, _)| Some(*p) != name_prop && !ignored.contains(p))
            .map(|(p, v)| format!("{p}={v}"))
            .collect::<Vec<_>>()
            .join(",");
        match id {
            QUARTZ_BLOCK => {
                let (n, v) = match self.expect("variant") {
                    "chiseled" => ("chiseled_quartz_block", "normal"),
                    "lines_y" => ("quartz_column", "axis=y"),
                    "lines_x" => ("quartz_column", "axis=x"),
                    "lines_z" => ("quartz_column", "axis=z"),
                    _ => ("quartz_block", "normal"),
                };
                name = n.into();
                variant = v.into();
            }
            DEADBUSH => {
                name = "dead_bush".into();
                variant = "normal".into();
            }
            DOUBLE_STONE_SLAB | DOUBLE_STONE_SLAB2 => {
                name = format!("{}_double_slab", self.expect("variant"));
                variant = if self.get_bool("seamless") { "all" } else { "normal" }.into();
            }
            _ => {}
        }
        if variant.is_empty() {
            variant = "normal".into();
        }
        Some(ModelLocation { name: format!("minecraft:{name}"), variant })
    }
}
