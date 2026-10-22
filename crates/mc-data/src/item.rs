//! `net.minecraft.item.Item` registry. The table is generated from the vanilla registry.

pub(crate) use crate::generated::items::ITEMS;

/// An instance field of the item's Java classes (e.g. `healAmount`, `damageVsEntity`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FieldValue {
    Num(f64),
    Bool(bool),
    Str(&'static str),
    List(&'static [&'static str]),
}

/// `Item.ToolMaterial`
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToolMaterial {
    pub name: &'static str,
    pub harvest_level: i32,
    pub max_uses: i32,
    pub efficiency: f32,
    pub damage: f32,
    pub enchantability: i32,
}

#[derive(Debug)]
pub struct Item {
    pub id: u16,
    pub name: &'static str,
    pub unlocalized: &'static str,
    /// `getItemStackLimit()`
    pub max_stack: u8,
    pub max_damage: u16,
    pub has_subtypes: bool,
    pub enchantability: u8,
    pub full3d: bool,
    /// `getContainerItem()` by registry name (bucket for milk, bottle for potions...).
    pub container: Option<&'static str>,
    /// Java class chain from the concrete class up to `Item`.
    pub classes: &'static [&'static str],
    /// The block of an `ItemBlock`, by registry name.
    pub block: Option<&'static str>,
    pub tool: Option<ToolMaterial>,
    pub fields: &'static [(&'static str, FieldValue)],
}

impl Item {
    pub fn all() -> &'static [Item] {
        &ITEMS
    }

    pub fn by_id(id: u16) -> Option<&'static Item> {
        static INDEX: std::sync::OnceLock<Vec<u16>> = std::sync::OnceLock::new();
        let index = INDEX.get_or_init(|| {
            let mut v = vec![u16::MAX; 32000];
            for (i, it) in ITEMS.iter().enumerate() {
                v[it.id as usize] = i as u16;
            }
            v
        });
        match index.get(id as usize) {
            Some(&i) if i != u16::MAX => Some(&ITEMS[i as usize]),
            _ => None,
        }
    }

    /// By registry name, with or without the `minecraft:` prefix.
    pub fn by_name(name: &str) -> Option<&'static Item> {
        static INDEX: std::sync::OnceLock<std::collections::HashMap<&'static str, u16>> = std::sync::OnceLock::new();
        let index = INDEX.get_or_init(|| ITEMS.iter().enumerate().map(|(i, it)| (it.name, i as u16)).collect());
        let full;
        let name = if name.contains(':') {
            name
        } else {
            full = format!("minecraft:{name}");
            &full
        };
        index.get(name).map(|&i| &ITEMS[i as usize])
    }

    pub fn is_a(&self, class: &str) -> bool {
        self.classes.contains(&class)
    }

    pub fn field(&self, name: &str) -> Option<FieldValue> {
        self.fields.iter().find(|(k, _)| *k == name).map(|(_, v)| *v)
    }

    pub fn num(&self, name: &str) -> f64 {
        match self.field(name) {
            Some(FieldValue::Num(n)) => n,
            other => panic!("{}: field {name} is {other:?}", self.name),
        }
    }

    /// `isDamageable()`: `maxDamage > 0 && !hasSubtypes`
    pub fn is_damageable(&self) -> bool {
        self.max_damage > 0 && !self.has_subtypes
    }
}
