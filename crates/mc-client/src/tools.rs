//! Mining rules: `Item.getStrVsBlock`, `canHarvestBlock` and the blocks' `getItemDropped`,
//! `quantityDropped` and `damageDropped`.
use mc_core::random::JavaRandom;
use mc_data::item::{FieldValue, Item};
use mc_data::{Block, Material, StateId};

fn class(item: &Item, name: &str) -> bool {
    item.classes.contains(&name)
}

fn effective_blocks(item: &Item) -> &'static [&'static str] {
    item.fields
        .iter()
        .find_map(|(k, v)| match (k, v) {
            (&"effectiveBlocks", FieldValue::List(l)) => Some(*l),
            _ => None,
        })
        .unwrap_or(&[])
}

/// `ItemStack.getStrVsBlock` (1 for an empty hand).
pub fn dig_speed(item: Option<&Item>, block: &Block) -> f32 {
    let Some(item) = item else { return 1.0 };
    let m = block.material;
    if class(item, "ItemSword") {
        return if block.name == "minecraft:web" {
            15.0
        } else if matches!(m, Material::Plants | Material::Vine | Material::Coral | Material::Leaves | Material::Gourd) {
            1.5
        } else {
            1.0
        };
    }
    if class(item, "ItemShears") {
        return if block.name == "minecraft:web" || m == Material::Leaves {
            15.0
        } else if block.name == "minecraft:wool" {
            5.0
        } else {
            1.0
        };
    }
    let Some(tool) = item.tool.filter(|_| class(item, "ItemTool")) else { return 1.0 };
    let proper = (class(item, "ItemPickaxe") && matches!(m, Material::Iron | Material::Anvil | Material::Rock))
        || (class(item, "ItemAxe") && matches!(m, Material::Wood | Material::Plants | Material::Vine))
        || effective_blocks(item).contains(&block.name);
    if proper {
        tool.efficiency
    } else {
        1.0
    }
}

/// `EntityPlayer.canHarvestBlock`
pub fn can_harvest(item: Option<&Item>, block: &Block) -> bool {
    if block.material.props().tool_not_required {
        return true;
    }
    let Some(item) = item else { return false };
    let n = block.name.trim_start_matches("minecraft:");
    if class(item, "ItemPickaxe") {
        let level = item.tool.map_or(0, |t| t.harvest_level);
        return match n {
            "obsidian" => level == 3,
            "diamond_block" | "diamond_ore" | "emerald_ore" | "emerald_block" | "gold_block" | "gold_ore" | "redstone_ore" | "lit_redstone_ore" => level >= 2,
            "iron_block" | "iron_ore" | "lapis_block" | "lapis_ore" => level >= 1,
            _ => matches!(block.material, Material::Rock | Material::Iron | Material::Anvil),
        };
    }
    if class(item, "ItemSpade") {
        return matches!(n, "snow_layer" | "snow");
    }
    if class(item, "ItemSword") {
        return n == "web";
    }
    if class(item, "ItemShears") {
        return matches!(n, "web" | "redstone_wire" | "tripwire");
    }
    false
}

/// `Block.dropBlockAsItem` contents: (item id, damage, count) stacks.
pub fn drops(state: StateId, rand: &mut JavaRandom) -> Vec<(u16, u16, u8)> {
    let block = state.block();
    let meta = state.meta() as u16;
    let n = block.name.trim_start_matches("minecraft:");
    let id = block.id;
    let one = |i: u16, m: u16| vec![(i, m, 1)];
    match n {
        "stone" => one(if meta == 0 { 4 } else { 1 }, meta),
        "grass" | "mycelium" | "farmland" => one(3, 0),
        "dirt" => one(3, if meta == 2 { 0 } else { meta }),
        "leaves" | "leaves2" => {
            let mut v = Vec::new();
            let variant = if n == "leaves" { meta & 3 } else { (meta & 3) + 4 };
            let chance = if variant == 3 { 40 } else { 20 };
            if rand.next_int_bounded(chance) == 0 {
                v.push((6, variant, 1));
            }
            if (variant == 0 || variant == 5) && rand.next_int_bounded(200) == 0 {
                v.push((260, 0, 1));
            }
            v
        }
        "tallgrass" => {
            if rand.next_int_bounded(8) == 0 {
                one(295, 0)
            } else {
                vec![]
            }
        }
        "deadbush" => {
            let c = rand.next_int_bounded(3) as u8;
            if c > 0 {
                vec![(280, 0, c)]
            } else {
                vec![]
            }
        }
        "glass" | "glass_pane" | "stained_glass" | "stained_glass_pane" | "ice" | "packed_ice" | "mob_spawner" | "fire" | "portal" | "end_portal"
        | "monster_egg" | "piston_extension" | "piston_head" | "bedrock" | "barrier" | "air" => vec![],
        "coal_ore" => one(263, 0),
        "diamond_ore" => one(264, 0),
        "emerald_ore" => one(388, 0),
        "quartz_ore" => one(406, 0),
        "lapis_ore" => vec![(351, 4, 4 + rand.next_int_bounded(5) as u8)],
        "redstone_ore" | "lit_redstone_ore" => vec![(331, 0, 4 + rand.next_int_bounded(2) as u8)],
        "gravel" => one(if rand.next_int_bounded(10) == 0 { 318 } else { 13 }, 0),
        "clay" => vec![(337, 0, 4)],
        "snow" => vec![(332, 0, 4)],
        "snow_layer" => vec![(332, 0, (meta & 7) as u8 + 1)],
        "glowstone" => vec![(348, 0, 2 + rand.next_int_bounded(3) as u8)],
        "melon_block" => vec![(360, 0, 3 + rand.next_int_bounded(5) as u8)],
        "bookshelf" => vec![(340, 0, 3)],
        "redstone_wire" => one(331, 0),
        "reeds" => one(338, 0),
        "wooden_door" => one(324, 0),
        "iron_door" => one(330, 0),
        "lit_redstone_lamp" => one(123, 0),
        "lit_furnace" => one(61, 0),
        "unpowered_repeater" | "powered_repeater" => one(356, 0),
        "unpowered_comparator" | "powered_comparator" => one(404, 0),
        "double_stone_slab" => vec![(44, meta & 7, 2)],
        "double_wooden_slab" => vec![(126, meta & 7, 2)],
        "double_stone_slab2" => vec![(182, meta & 7, 2)],
        "wool" | "carpet" | "planks" | "sand" | "sandstone" | "red_sandstone" | "stonebrick" | "stained_hardened_clay" | "cobblestone_wall"
        | "prismarine" | "red_flower" | "sponge" | "silver_glazed" => one(id, meta),
        "sapling" => one(id, meta & 7),
        "log" | "log2" => one(id, meta & 3),
        "stone_slab" | "wooden_slab" | "stone_slab2" => one(id, meta & 7),
        "quartz_block" => one(id, if meta >= 2 { 2 } else { meta }),
        "anvil" => one(id, meta >> 2),
        _ => one(id, 0),
    }
}
