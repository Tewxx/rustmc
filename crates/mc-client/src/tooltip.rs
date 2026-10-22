//! `ItemStack.getTooltip` and `GuiScreen.renderToolTip`: tooltip lines with formatting codes.
use crate::inventory::Catalogue;
use mc_core::nbt::{Compound, Tag};
use mc_gui::lang::Lang;
use mc_gui::ItemStack;

/// `Item.itemModifierUUID`
const ITEM_MODIFIER_UUID: (i64, i64) = (0xCB3F55D3_645C_4F38u64 as i64, 0xA497_9C13A33DB5CFu64 as i64);

fn compound<'a>(c: &'a Compound, key: &str) -> Option<&'a Compound> {
    match c.get(key) {
        Some(Tag::Compound(c)) => Some(c),
        _ => None,
    }
}

fn list<'a>(c: &'a Compound, key: &str, ty: u8) -> &'a [Tag] {
    match c.get(key) {
        Some(Tag::List(t, v)) if *t == ty || v.is_empty() => v,
        _ => &[],
    }
}

/// `NBTTagCompound.getInteger` and friends: any numeric tag, else 0.
pub fn num(c: &Compound, key: &str) -> f64 {
    match c.get(key) {
        Some(Tag::Byte(v)) => *v as f64,
        Some(Tag::Short(v)) => *v as f64,
        Some(Tag::Int(v)) => *v as f64,
        Some(Tag::Long(v)) => *v as f64,
        Some(Tag::Float(v)) => *v as f64,
        Some(Tag::Double(v)) => *v,
        _ => 0.0,
    }
}

fn string<'a>(c: &'a Compound, key: &str) -> Option<&'a str> {
    match c.get(key) {
        Some(Tag::String(s)) => Some(s),
        _ => None,
    }
}

/// `ItemStack.getEnchantmentTagList` (or `StoredEnchantments`) as `(id, level)`.
pub fn enchantments(stack: &ItemStack, key: &str) -> Vec<(i32, i32)> {
    let Some(tag) = stack.tag.map(|t| t.get()) else { return vec![] };
    list(&tag, key, 10)
        .iter()
        .filter_map(|t| match t {
            Tag::Compound(e) => Some((num(e, "id") as i16 as i32, num(e, "lvl") as i16 as i32)),
            _ => None,
        })
        .collect()
}

/// `EnchantmentHelper.getEnchantmentLevel`
pub fn enchantment_level(stack: Option<&ItemStack>, id: i32) -> i32 {
    stack.map_or(0, |s| enchantments(s, "ench").iter().find(|e| e.0 == id).map_or(0, |e| e.1))
}

/// `isItemStackDamageable`: a max damage and no `Unbreakable` tag.
pub fn damageable(cat: &Catalogue, stack: &ItemStack) -> bool {
    let max = mc_data::item::Item::by_id(cat.items[stack.item].id).map_or(0, |i| i.max_damage);
    max > 0 && !stack.tag.is_some_and(|t| num(&t.get(), "Unbreakable") != 0.0)
}

/// `isItemDamaged`
pub fn is_damaged(cat: &Catalogue, stack: &ItemStack) -> bool {
    damageable(cat, stack) && stack.damage > 0
}

/// `getRarity().rarityColor` as a formatting code.
pub fn rarity_color(cat: &Catalogue, stack: &ItemStack) -> char {
    let id = cat.items[stack.item].id;
    let enchanted = !enchantments(stack, "ench").is_empty();
    match id {
        // ItemAppleGold
        322 => {
            if stack.damage == 0 {
                'b'
            } else {
                'd'
            }
        }
        // ItemRecord
        2256..=2267 => 'b',
        // ItemEnchantedBook
        403 if !enchantments(stack, "StoredEnchantments").is_empty() => 'e',
        _ if enchanted => 'b',
        _ => 'f',
    }
}

/// `ItemStack.getDisplayName`
pub fn display_name(cat: &Catalogue, stack: &ItemStack) -> String {
    stack
        .tag
        .and_then(|t| compound(&t.get(), "display").and_then(|d| string(d, "Name")).map(str::to_string))
        .unwrap_or_else(|| cat.items[stack.item].title.clone())
}

fn has_display_name(stack: &ItemStack) -> bool {
    stack.tag.is_some_and(|t| compound(&t.get(), "display").is_some_and(|d| string(d, "Name").is_some()))
}

/// `DecimalFormat("#.###")`
fn decimal(d: f64) -> String {
    let s = format!("{d:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".into()
    } else {
        s.into()
    }
}

/// One attribute modifier line set: `(attribute name, uuid, amount, operation)`.
fn attribute_modifiers(cat: &Catalogue, stack: &ItemStack) -> Vec<(String, (i64, i64), f64, i32)> {
    if let Some(t) = stack.tag.map(|t| t.get()) {
        if let Some(Tag::List(_, l)) = t.get("AttributeModifiers") {
            return l
                .iter()
                .filter_map(|m| match m {
                    Tag::Compound(m) if num(m, "Amount").is_finite() => Some((
                        string(m, "AttributeName")?.to_string(),
                        (num(m, "UUIDMost") as i64, num(m, "UUIDLeast") as i64),
                        num(m, "Amount"),
                        num(m, "Operation") as i32,
                    )),
                    _ => None,
                })
                .collect();
        }
    }
    let Some(item) = mc_data::item::Item::by_id(cat.items[stack.item].id) else { return vec![] };
    // ItemSword "Weapon modifier" / ItemTool "Tool modifier".
    let amount = if item.is_a("ItemSword") {
        item.num("attackDamage")
    } else if item.is_a("ItemTool") {
        item.num("damageVsEntity")
    } else {
        return vec![];
    };
    vec![("generic.attackDamage".into(), ITEM_MODIFIER_UUID, amount, 0)]
}

/// `EnchantmentHelper.getModifierForCreature(stack, UNDEFINED)`: Sharpness only.
fn sharpness_bonus(stack: &ItemStack) -> f32 {
    enchantments(stack, "ench").iter().filter(|e| e.0 == 16).map(|e| e.1 as f32 * 1.25).sum()
}

/// `ItemStack.getTooltip(player, advanced)`
pub fn lines(cat: &Catalogue, lang: &Lang, stack: &ItemStack, advanced: bool) -> Vec<String> {
    let def = &cat.items[stack.item];
    let reg = mc_data::item::Item::by_id(def.id);
    let tag = stack.tag.map(|t| t.get());
    let mut list = Vec::new();
    let mut s = display_name(cat, stack);
    let named = has_display_name(stack);
    if named {
        s = format!("\u{a7}o{s}");
    }
    s.push_str("\u{a7}r");
    if advanced {
        let close = if s.is_empty() { "" } else { ")" };
        if !s.is_empty() {
            s.push_str(" (");
        }
        if reg.is_some_and(|r| r.has_subtypes) {
            s.push_str(&format!("#{:04}/{}{close}", def.id, stack.damage));
        } else {
            s.push_str(&format!("#{:04}{close}", def.id));
        }
    } else if !named && def.id == 358 {
        s.push_str(&format!(" #{}", stack.damage));
    }
    list.push(s);
    let hide = tag.as_deref().map_or(0, |t| num(t, "HideFlags") as i32);
    if hide & 32 == 0 {
        add_information(lang, def.id, stack, tag.as_deref(), &mut list);
    }
    if let Some(t) = tag.as_deref() {
        if hide & 1 == 0 {
            for (id, lvl) in enchantments(stack, "ench") {
                if let Some(e) = mc_data::Enchantment::by_id(id) {
                    list.push(enchantment_name(lang, e.name, lvl));
                }
            }
        }
        if let Some(d) = compound(t, "display") {
            if matches!(d.get("color"), Some(Tag::Int(_))) {
                if advanced {
                    list.push(format!("Color: #{:X}", num(d, "color") as i32));
                } else {
                    list.push(format!("\u{a7}o{}", lang.get("item.dyed")));
                }
            }
            for l in list_strings(d, "Lore") {
                list.push(format!("\u{a7}5\u{a7}o{l}"));
            }
        }
    }
    let mods = attribute_modifiers(cat, stack);
    if !mods.is_empty() && hide & 2 == 0 {
        list.push(String::new());
        for (name, uuid, amount, op) in mods {
            let mut d0 = amount;
            if uuid == ITEM_MODIFIER_UUID {
                d0 += sharpness_bonus(stack) as f64;
            }
            let mut d1 = if op != 1 && op != 2 { d0 } else { d0 * 100.0 };
            let attr = lang.get(&format!("attribute.name.{name}"));
            if d0 > 0.0 {
                list.push(format!("\u{a7}9{}", lang.format(&format!("attribute.modifier.plus.{op}"), &[&decimal(d1), &attr])));
            } else if d0 < 0.0 {
                d1 *= -1.0;
                list.push(format!("\u{a7}c{}", lang.format(&format!("attribute.modifier.take.{op}"), &[&decimal(d1), &attr])));
            }
        }
    }
    if let Some(t) = tag.as_deref() {
        if num(t, "Unbreakable") != 0.0 && hide & 4 == 0 {
            list.push(format!("\u{a7}9{}", lang.get("item.unbreakable")));
        }
        for (key, flag, title) in [("CanDestroy", 8, "item.canBreak"), ("CanPlaceOn", 16, "item.canPlace")] {
            let names = list_strings(t, key);
            if hide & flag == 0 && !names.is_empty() {
                list.push(String::new());
                list.push(format!("\u{a7}7{}", lang.get(title)));
                for n in names {
                    let name = mc_data::Block::by_name(&if n.contains(':') { n.clone() } else { format!("minecraft:{n}") })
                        .map_or_else(|| "missingno".to_string(), |b| lang.get(&format!("tile.{}.name", block_key(b.name))));
                    list.push(format!("\u{a7}8{name}"));
                }
            }
        }
    }
    if advanced {
        if is_damaged(cat, stack) {
            let max = reg.map_or(0, |r| r.max_damage as i32);
            list.push(format!("Durability: {} / {max}", max - stack.damage as i32));
        }
        list.push(format!("\u{a7}8{}", reg.map_or("minecraft:air", |r| r.name)));
        if let Some(t) = tag.as_deref() {
            list.push(format!("\u{a7}8NBT: {} tag(s)", t.len()));
        }
    }
    list
}

/// The unlocalized block name lives in the language keys; fall back to the registry path.
fn block_key(name: &str) -> String {
    mc_data::item::Item::by_name(name).map_or_else(|| name.trim_start_matches("minecraft:").to_string(), |i| i.unlocalized.trim_start_matches("tile.").to_string())
}

fn list_strings(c: &Compound, key: &str) -> Vec<String> {
    list(c, key, 8)
        .iter()
        .filter_map(|t| match t {
            Tag::String(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

/// `Enchantment.getTranslatedName`
pub fn enchantment_name(lang: &Lang, key: &str, level: i32) -> String {
    format!("{} {}", lang.get(key), lang.get(&format!("enchantment.level.{level}")))
}

/// `Item.addInformation` for the items that override it (potions are not modelled yet).
fn add_information(lang: &Lang, id: u16, stack: &ItemStack, tag: Option<&Compound>, list: &mut Vec<String>) {
    match id {
        // ItemEnchantedBook
        403 => {
            for (eid, lvl) in enchantments(stack, "StoredEnchantments") {
                if let Some(e) = mc_data::Enchantment::by_id(eid) {
                    list.push(enchantment_name(lang, e.name, lvl));
                }
            }
        }
        // ItemEditableBook
        387 => {
            if let Some(t) = tag {
                if let Some(a) = string(t, "author").filter(|a| !a.is_empty()) {
                    list.push(format!("\u{a7}7{}", lang.format("book.byAuthor", &[a])));
                }
                list.push(format!("\u{a7}7{}", lang.get(&format!("book.generation.{}", num(t, "generation") as i32))));
            }
        }
        // ItemRecord.getRecordNameLocal
        2256..=2267 => {
            let name = mc_data::item::Item::by_id(id).map_or("", |i| i.name.trim_start_matches("minecraft:record_"));
            list.push(lang.get(&format!("item.record.{name}.desc")));
        }
        _ => {}
    }
}

/// `GuiScreen.renderToolTip`: rarity color on the first line, gray on the rest.
pub fn render_lines(cat: &Catalogue, lang: &Lang, stack: &ItemStack, advanced: bool) -> Vec<String> {
    let rarity = rarity_color(cat, stack);
    lines(cat, lang, stack, advanced)
        .into_iter()
        .enumerate()
        .map(|(i, l)| if i == 0 { format!("\u{a7}{rarity}{l}") } else { format!("\u{a7}7{l}") })
        .collect()
}
