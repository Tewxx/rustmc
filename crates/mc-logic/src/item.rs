//! `ItemStack`, chest loot (`WeightedRandomChestContent`) and random enchanting
//! (`EnchantmentHelper.buildEnchantmentList`), drawing randoms in vanilla's order.

use mc_core::nbt::{Compound, Tag};
use mc_core::JavaRandom;
use mc_data::{Enchantment, Item};

#[derive(Clone, Debug, PartialEq)]
pub struct ItemStack {
    pub item: u16,
    pub count: i32,
    pub damage: i32,
    pub tag: Option<Compound>,
}

impl ItemStack {
    pub fn new(item: u16, count: i32, damage: i32) -> Self {
        Self { item, count, damage, tag: None }
    }

    pub fn by_name(name: &str, count: i32, damage: i32) -> Self {
        let it = Item::by_name(name).unwrap_or_else(|| panic!("no item {name}"));
        Self::new(it.id, count, damage)
    }

    pub fn def(&self) -> &'static Item {
        Item::by_id(self.item).expect("unknown item id")
    }

    /// `getMaxStackSize`
    pub fn max_stack_size(&self) -> i32 {
        self.def().max_stack as i32
    }

    /// `writeToNBT` (without the slot byte).
    pub fn to_nbt(&self) -> Compound {
        let mut c = Compound::new();
        c.insert("id".into(), Tag::String(self.def().name.to_string()));
        c.insert("Count".into(), Tag::Byte(self.count as i8));
        c.insert("Damage".into(), Tag::Short(self.damage as i16));
        if let Some(t) = &self.tag {
            c.insert("tag".into(), Tag::Compound(t.clone()));
        }
        c
    }

    /// `loadItemStackFromNBT`
    pub fn from_nbt(c: &Compound) -> Option<Self> {
        let item = match c.get("id")? {
            Tag::String(s) => Item::by_name(s)?.id,
            Tag::Short(n) => *n as u16,
            _ => return None,
        };
        let count = match c.get("Count") {
            Some(Tag::Byte(b)) => *b as i32,
            _ => 1,
        };
        let damage = match c.get("Damage") {
            Some(Tag::Short(s)) => (*s).max(0) as i32,
            _ => 0,
        };
        let tag = match c.get("tag") {
            Some(Tag::Compound(t)) => Some(t.clone()),
            _ => None,
        };
        Some(Self { item, count, damage, tag })
    }
}

/// Iteration order of a `java.util.HashMap<Integer, _>` holding `keys` (in insertion order,
/// no removals): buckets by `key & (capacity - 1)`, insertion order within a bucket.
pub fn java_int_hashmap_order(keys: &[i32]) -> Vec<i32> {
    let mut cap = 16usize;
    while keys.len() as f64 > cap as f64 * 0.75 {
        cap *= 2;
    }
    let mut v: Vec<(usize, usize, i32)> = keys
        .iter()
        .enumerate()
        .map(|(i, &k)| {
            let h = k ^ ((k as u32) >> 16) as i32;
            ((h as usize) & (cap - 1), i, k)
        })
        .collect();
    v.sort();
    v.into_iter().map(|(_, _, k)| k).collect()
}

#[derive(Clone, Copy, Debug)]
pub struct EnchantmentData {
    pub ench: &'static Enchantment,
    pub level: i32,
}

/// `EnchantmentHelper.mapEnchantmentData`, as a list in the map's iteration order.
fn map_enchantment_data(level: i32, stack: &ItemStack) -> Vec<EnchantmentData> {
    let is_book = stack.def().name == "minecraft:book";
    let mut entries: Vec<EnchantmentData> = Vec::new();
    for e in Enchantment::all() {
        if is_book || e.table_items.contains(&stack.item) {
            for i in e.min_level..=e.max_level {
                if level >= e.min_enchantability[(i - 1) as usize] && level <= e.max_enchantability[(i - 1) as usize] {
                    match entries.iter_mut().find(|d| d.ench.id == e.id) {
                        Some(d) => d.level = i,
                        None => entries.push(EnchantmentData { ench: e, level: i }),
                    }
                }
            }
        }
    }
    let order = java_int_hashmap_order(&entries.iter().map(|d| d.ench.id as i32).collect::<Vec<_>>());
    order.iter().map(|k| *entries.iter().find(|d| d.ench.id as i32 == *k).unwrap()).collect()
}

fn pick_weighted<'a>(rand: &mut JavaRandom, list: &'a [EnchantmentData]) -> &'a EnchantmentData {
    let total: i32 = list.iter().map(|d| d.ench.weight).sum();
    let mut w = rand.next_int_bounded(total);
    for d in list {
        w -= d.ench.weight;
        if w < 0 {
            return d;
        }
    }
    unreachable!()
}

/// `EnchantmentHelper.buildEnchantmentList`
pub fn build_enchantment_list(rand: &mut JavaRandom, stack: &ItemStack, level: i32) -> Option<Vec<EnchantmentData>> {
    let mut i = stack.def().enchantability as i32;
    if i <= 0 {
        return None;
    }
    i /= 2;
    let a = rand.next_int_bounded((i >> 1) + 1);
    let b = rand.next_int_bounded((i >> 1) + 1);
    i = 1 + a + b;
    let j = i + level;
    let f = (rand.next_float() + rand.next_float() - 1.0) * 0.15;
    let mut k = (j as f32 * (1.0 + f) + 0.5) as i32;
    if k < 1 {
        k = 1;
    }
    let mut map = map_enchantment_data(k, stack);
    if map.is_empty() {
        return None;
    }
    let first = *pick_weighted(rand, &map);
    let mut list = vec![first];
    let mut l = k;
    while rand.next_int_bounded(50) <= l {
        map.retain(|d| list.iter().all(|e| e.ench.compatible.contains(&d.ench.id)));
        if !map.is_empty() {
            list.push(*pick_weighted(rand, &map));
        }
        l >>= 1;
    }
    Some(list)
}

/// `EnchantmentHelper.addRandomEnchantment`
pub fn add_random_enchantment(rand: &mut JavaRandom, stack: &mut ItemStack, level: i32) {
    let list = build_enchantment_list(rand, stack, level);
    let is_book = stack.def().name == "minecraft:book";
    if is_book {
        stack.item = Item::by_name("minecraft:enchanted_book").unwrap().id;
    }
    let key = if is_book { "StoredEnchantments" } else { "ench" };
    if let Some(list) = list {
        for d in list {
            let tag = stack.tag.get_or_insert_with(Compound::new);
            let entry = tag.entry(key.to_string()).or_insert_with(|| Tag::List(10, Vec::new()));
            let Tag::List(_, items) = entry else { unreachable!() };
            let mut found = false;
            for e in items.iter_mut() {
                if let Tag::Compound(c) = e {
                    if c.get("id") == Some(&Tag::Short(d.ench.id as i16)) {
                        if is_book {
                            if let Some(Tag::Short(lvl)) = c.get("lvl") {
                                if (*lvl as i32) < d.level {
                                    c.insert("lvl".into(), Tag::Short(d.level as i16));
                                }
                            }
                            found = true;
                            break;
                        }
                    }
                }
            }
            if !found {
                let mut c = Compound::new();
                c.insert("id".into(), Tag::Short(d.ench.id as i16));
                c.insert("lvl".into(), Tag::Short(d.level as i16));
                items.push(Tag::Compound(c));
            }
        }
    }
}

/// `WeightedRandomChestContent`
#[derive(Clone, Debug)]
pub struct ChestContent {
    pub stack: ItemStack,
    pub min: i32,
    pub max: i32,
    pub weight: i32,
}

impl ChestContent {
    pub fn new(item: &str, damage: i32, min: i32, max: i32, weight: i32) -> Self {
        Self { stack: ItemStack::by_name(item, 1, damage), min, max, weight }
    }
}

/// `ItemEnchantedBook.getRandom(rand)`: an enchanted book entry, weight 1.
pub fn random_enchanted_book(rand: &mut JavaRandom, min: i32, max: i32, weight: i32) -> ChestContent {
    let mut stack = ItemStack::by_name("minecraft:book", 1, 0);
    add_random_enchantment(rand, &mut stack, 30);
    ChestContent { stack, min, max, weight }
}

/// `generateChestContents`: fills `slots` (an inventory of `slots.len()`).
pub fn generate_chest_contents(rand: &mut JavaRandom, list: &[ChestContent], slots: &mut [Option<ItemStack>], max: i32) {
    let total: i32 = list.iter().map(|c| c.weight).sum();
    for _ in 0..max {
        let mut w = rand.next_int_bounded(total);
        let mut pick = &list[0];
        for c in list {
            w -= c.weight;
            if w < 0 {
                pick = c;
                break;
            }
        }
        let j = pick.min + rand.next_int_bounded(pick.max - pick.min + 1);
        if pick.stack.max_stack_size() >= j {
            let mut s = pick.stack.clone();
            s.count = j;
            let slot = rand.next_int_bounded(slots.len() as i32) as usize;
            slots[slot] = Some(s);
        } else {
            for _ in 0..j {
                let mut s = pick.stack.clone();
                s.count = 1;
                let slot = rand.next_int_bounded(slots.len() as i32) as usize;
                slots[slot] = Some(s);
            }
        }
    }
}

/// Inventory slots as the `Items` list of a container tile entity.
pub fn slots_to_nbt(slots: &[Option<ItemStack>]) -> Tag {
    let mut list = Vec::new();
    for (i, s) in slots.iter().enumerate() {
        if let Some(s) = s {
            let mut c = s.to_nbt();
            c.insert("Slot".into(), Tag::Byte(i as i8));
            list.push(Tag::Compound(c));
        }
    }
    Tag::List(10, list)
}

/// The `Items` list of a container tile entity as slots.
pub fn slots_from_nbt(tag: Option<&Tag>, size: usize) -> Vec<Option<ItemStack>> {
    let mut slots = vec![None; size];
    if let Some(Tag::List(_, list)) = tag {
        for t in list {
            if let Tag::Compound(c) = t {
                if let Some(Tag::Byte(slot)) = c.get("Slot") {
                    let slot = (*slot as i32 & 255) as usize;
                    if slot < size {
                        slots[slot] = ItemStack::from_nbt(c);
                    }
                }
            }
        }
    }
    slots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashmap_order_matches_java() {
        // Java: new HashMap<Integer,..>() with 0,32,1,33 → 0,32,1,33 (cap 16: buckets 0,0,1,1).
        assert_eq!(java_int_hashmap_order(&[33, 1, 32, 0]), vec![32, 0, 33, 1]);
        assert_eq!(java_int_hashmap_order(&[48, 16, 1]), vec![48, 16, 1]);
    }
}
