//! `net.minecraft.enchantment.Enchantment` table, generated from the vanilla registry.

pub(crate) use crate::generated::enchantments::ENCHANTMENTS;

#[derive(Debug)]
pub struct Enchantment {
    pub id: u8,
    /// Translation key, e.g. `enchantment.protect.all`.
    pub name: &'static str,
    pub class: &'static str,
    pub weight: i32,
    /// `EnumEnchantmentType` name.
    pub kind: &'static str,
    pub min_level: i32,
    pub max_level: i32,
    /// `getMinEnchantability(level)` for levels 1..=max (index level - 1).
    pub min_enchantability: &'static [i32],
    pub max_enchantability: &'static [i32],
    /// Ids `canApplyTogether(other)` accepts (one direction, as vanilla calls it).
    pub compatible: &'static [u8],
    /// Item ids `type.canEnchantItem` accepts (enchanting table).
    pub table_items: &'static [u16],
    /// Item ids `canApply` accepts (anvil).
    pub anvil_items: &'static [u16],
}

impl Enchantment {
    /// `Enchantment.enchantmentsBookList` order.
    pub fn all() -> &'static [Enchantment] {
        &ENCHANTMENTS
    }

    pub fn by_id(id: i32) -> Option<&'static Enchantment> {
        ENCHANTMENTS.iter().find(|e| e.id as i32 == id)
    }
}
