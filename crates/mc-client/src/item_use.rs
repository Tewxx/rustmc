//! `Item.getItemUseAction`, `getMaxItemUseDuration` and the client half of `onItemRightClick`.
use mc_data::item::Item;

/// `EnumAction`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Eat,
    Drink,
    Block,
    Bow,
}

impl Action {
    /// Index passed to the renderer (0 none … 4 bow).
    pub fn index(self) -> u8 {
        self as u8
    }
}

/// `(getItemUseAction, getMaxItemUseDuration)`
pub fn action(id: u16) -> (Action, i32) {
    let Some(item) = Item::by_id(id) else { return (Action::None, 0) };
    if item.is_a("ItemSword") {
        (Action::Block, 72000)
    } else if item.is_a("ItemBow") {
        (Action::Bow, 72000)
    } else if item.is_a("ItemFood") {
        (Action::Eat, item.num("itemUseDuration") as i32)
    } else if item.is_a("ItemPotion") || item.is_a("ItemBucketMilk") {
        (Action::Drink, 32)
    } else {
        (Action::None, 0)
    }
}

/// What `onItemRightClick` does on the client for the held stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RightClick {
    /// `setItemInUse(stack, duration)`
    StartUsing,
    /// `--stackSize` (throwables; the server spawns the entity).
    Consume,
    /// `ItemArmor`: move a copy into this empty armor slot (0 = boots … 3 = helmet), emptying the stack.
    Equip(usize),
    /// `ItemFishingRod`: swings the arm.
    Swing,
    Nothing,
}

/// `EntityLiving.getArmorPosition(stack) - 1` as an armor inventory index (0 boots … 3 helmet).
pub fn armor_slot(id: u16) -> Option<usize> {
    let item = Item::by_id(id)?;
    if id == 397 || id == 86 {
        // Skulls and pumpkins go on the head.
        return Some(3);
    }
    if !item.is_a("ItemArmor") {
        return None;
    }
    // armorType: 0 helmet, 1 chestplate, 2 leggings, 3 boots.
    Some(3 - item.num("armorType") as usize)
}

pub struct Context {
    pub creative: bool,
    pub food_level: i32,
    pub has_arrow: bool,
    /// `getCurrentArmor(i)` is empty, indexed boots … helmet.
    pub armor_empty: [bool; 4],
}

/// `Item.onItemRightClick` (client side) for item `id` with metadata `damage`.
pub fn right_click(id: u16, damage: i16, cx: &Context) -> RightClick {
    let Some(item) = Item::by_id(id) else { return RightClick::Nothing };
    if item.is_a("ItemSword") || item.is_a("ItemBucketMilk") {
        return RightClick::StartUsing;
    }
    if item.is_a("ItemFood") {
        // canEat: (alwaysEdible || needFood) && !disableDamage
        let always = matches!(item.field("alwaysEdible"), Some(mc_data::item::FieldValue::Bool(true)));
        return if (always || cx.food_level < 20) && !cx.creative { RightClick::StartUsing } else { RightClick::Nothing };
    }
    if item.is_a("ItemPotion") {
        // ItemPotion.isSplash
        return if damage as i32 & 16384 != 0 {
            if cx.creative {
                RightClick::Nothing
            } else {
                RightClick::Consume
            }
        } else {
            RightClick::StartUsing
        };
    }
    if item.is_a("ItemBow") {
        return if cx.creative || cx.has_arrow { RightClick::StartUsing } else { RightClick::Nothing };
    }
    match id {
        // ItemSnowball, ItemEgg, ItemExpBottle: creative keeps the stack.
        332 | 344 | 384 => {
            if cx.creative {
                RightClick::Nothing
            } else {
                RightClick::Consume
            }
        }
        // ItemEnderPearl does nothing at all in creative.
        368 => {
            if cx.creative {
                RightClick::Nothing
            } else {
                RightClick::Consume
            }
        }
        346 => RightClick::Swing,
        _ => match armor_slot(id) {
            // ItemArmor only (skulls/pumpkins are placed or worn through the inventory).
            Some(slot) if item.is_a("ItemArmor") && cx.armor_empty[slot] => RightClick::Equip(slot),
            _ => RightClick::Nothing,
        },
    }
}

/// `Block.onBlockActivated` returning true on the client (the GUI and toggle blocks), which stops
/// the held item from being used on the block.
pub fn activates(block: &mc_data::Block) -> bool {
    matches!(
        block.name.trim_start_matches("minecraft:"),
        "chest"
            | "trapped_chest"
            | "ender_chest"
            | "crafting_table"
            | "furnace"
            | "lit_furnace"
            | "dispenser"
            | "dropper"
            | "hopper"
            | "brewing_stand"
            | "enchanting_table"
            | "anvil"
            | "beacon"
            | "bed"
            | "noteblock"
            | "lever"
            | "stone_button"
            | "wooden_button"
            | "wooden_door"
            | "spruce_door"
            | "birch_door"
            | "jungle_door"
            | "acacia_door"
            | "dark_oak_door"
            | "trapdoor"
            | "fence_gate"
            | "spruce_fence_gate"
            | "birch_fence_gate"
            | "jungle_fence_gate"
            | "dark_oak_fence_gate"
            | "acacia_fence_gate"
            | "unpowered_repeater"
            | "powered_repeater"
            | "unpowered_comparator"
            | "powered_comparator"
            | "cake"
            | "command_block"
            | "daylight_detector"
            | "daylight_detector_inverted"
            | "dragon_egg"
    )
}
