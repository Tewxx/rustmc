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

/// `ItemStack.isItemEnchanted`: an `ench` list tag, even an empty one.
pub fn is_enchanted(stack: &ItemStack) -> bool {
    stack.tag.is_some_and(|t| matches!(t.get().get("ench"), Some(Tag::List(..))))
}

/// `ItemStack.hasEffect` (`Item.hasEffect` overrides): the enchantment glint.
pub fn has_effect(cat: &Catalogue, stack: &ItemStack) -> bool {
    match cat.items[stack.item].id {
        // ItemAppleGold: the enchanted apple
        322 => stack.damage > 0,
        // ItemEnchantedBook, ItemSimpleFoiled (experience bottle, nether star)
        403 | 384 | 399 => true,
        // ItemPotion: any effects
        373 => !potion_effects(stack).is_empty(),
        _ => is_enchanted(stack),
    }
}

/// `ItemPotion.getEffects(stack)` as `(id, duration, amplifier, showParticles)`: a
/// `CustomPotionEffects` list replaces the damage-value effects.
pub fn potion_effects(stack: &ItemStack) -> Vec<(u8, i32, i32, bool)> {
    if let Some(t) = stack.tag.map(|t| t.get()) {
        if matches!(t.get("CustomPotionEffects"), Some(Tag::List(..))) {
            // PotionEffect.readCustomPotionEffectFromNBT
            return list(&t, "CustomPotionEffects", 10)
                .iter()
                .filter_map(|e| match e {
                    Tag::Compound(e) => {
                        let id = num(e, "Id") as i64 as i8;
                        let particles = !matches!(e.get("ShowParticles"), Some(Tag::Byte(0)));
                        (id >= 0 && mc_data::potion::by_id(id as u8).is_some()).then(|| (id as u8, num(e, "Duration") as i64 as i32, num(e, "Amplifier") as i64 as i8 as i32, particles))
                    }
                    _ => None,
                })
                .collect();
        }
    }
    mc_data::potion::effects(stack.damage as i32).into_iter().map(|b| (b.id, b.duration, b.amplifier, true)).collect()
}

/// `ItemPotion.getItemStackDisplayName`
fn potion_name(lang: &Lang, stack: &ItemStack) -> String {
    let meta = stack.damage as i32;
    if meta == 0 {
        return lang.get("item.emptyPotion.name").trim().to_string();
    }
    let mut s = String::new();
    if mc_data::potion::is_splash(meta) {
        s = format!("{} ", lang.get("potion.prefix.grenade").trim());
    }
    match potion_effects(stack).first().and_then(|e| mc_data::potion::by_id(e.0)) {
        Some(p) => s + lang.get(&format!("{}.postfix", p.name)).trim(),
        None => format!("{} {}", lang.get(mc_data::potion::prefix(meta)).trim(), lang.get("item.potion.name").trim()),
    }
}

/// `ItemStack.getDisplayName`, with `renderSelectedItem`'s italic for custom names.
pub fn highlight_name(cat: &Catalogue, lang: &Lang, stack: &ItemStack) -> String {
    if has_display_name(stack) {
        format!("\u{a7}o{}", display_name(cat, stack))
    } else if cat.items[stack.item].id == 373 {
        potion_name(lang, stack)
    } else {
        display_name(cat, stack)
    }
}

/// `ItemStack.areItemStackTagsEqual`
pub fn tags_equal(a: &ItemStack, b: &ItemStack) -> bool {
    match (a.tag, b.tag) {
        (None, None) => true,
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// `StringUtils.ticksToElapsedTime`
fn elapsed(ticks: i32) -> String {
    let s = ticks / 20;
    let (m, s) = (s / 60, s % 60);
    if s < 10 {
        format!("{m}:0{s}")
    } else {
        format!("{m}:{s}")
    }
}

/// `ItemPotion.addInformation`
fn potion_information(lang: &Lang, stack: &ItemStack, list: &mut Vec<String>) {
    use mc_core::java_hash::{string_hash, uuid_hash, JavaHashMap};
    if stack.damage == 0 {
        return;
    }
    let effects = potion_effects(stack);
    // HashMultimap<String, AttributeModifier>: a HashMap of HashSets keyed by modifier UUID.
    let mut multimap: JavaHashMap<&str, JavaHashMap<(u64, u64), (f64, i32)>> = JavaHashMap::new();
    if effects.is_empty() {
        list.push(format!("\u{a7}7{}", lang.get("potion.empty").trim()));
    }
    for &(id, duration, amp, _) in &effects {
        let Some(p) = mc_data::potion::by_id(id) else { continue };
        let mut s1 = lang.get(p.name).trim().to_string();
        // Potion.attributeModifierMap with getAttributeModifierAmount (PotionAttackDamage overrides).
        let modifier: Option<(&str, (u64, u64), f64, i32)> = match id {
            1 => Some(("generic.movementSpeed", (0x91AEAA56376B4498, 0x935B2F7F68070635), 0.20000000298023224 * (amp + 1) as f64, 2)),
            2 => Some(("generic.movementSpeed", (0x7107DE5E7CE84030, 0x940E514C1F160890), -0.15000000596046448 * (amp + 1) as f64, 2)),
            5 => Some(("generic.attackDamage", (0x648D70646A604F59, 0x8ABEC2C23A6DD7A9), 1.3 * (amp + 1) as f64, 2)),
            18 => Some(("generic.attackDamage", (0x22653B89116E49DC, 0x9B6B9971489B5BE5), (-0.5f32 * (amp + 1) as f32) as f64, 0)),
            21 => Some(("generic.maxHealth", (0x5D6F0BA2118646AC, 0xB896C61C5CEE99CC), 4.0 * (amp + 1) as f64, 0)),
            _ => None,
        };
        if let Some((name, uuid, amount, op)) = modifier {
            if !multimap.contains_key(&name) {
                multimap.insert(string_hash(name), name, JavaHashMap::with_initial(3));
            }
            let set = multimap.get_mut(&name).unwrap();
            if !set.contains_key(&uuid) {
                set.insert(uuid_hash(uuid.0, uuid.1), uuid, (amount, op));
            }
        }
        if amp > 0 {
            s1 = format!("{s1} {}", lang.get(&format!("potion.potency.{amp}")).trim());
        }
        if duration > 20 {
            s1 = format!("{s1} ({})", elapsed(duration));
        }
        list.push(format!("\u{a7}{}{s1}", if p.bad { 'c' } else { '7' }));
    }
    if !multimap.is_empty() {
        list.push(String::new());
        list.push(format!("\u{a7}5{}", lang.get("potion.effects.whenDrank")));
        for (name, set) in multimap.iter() {
            for (_, &(d0, op)) in set.iter() {
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
    }
}

/// `ItemArmor.getColor` for leather with a `display.color` tag.
pub fn armor_dye(cat: &Catalogue, stack: &ItemStack) -> Option<u32> {
    if !(298..=301).contains(&cat.items[stack.item].id) {
        return None;
    }
    stack.tag.and_then(|t| {
        compound(&t.get(), "display").and_then(|d| match d.get("color") {
            Some(Tag::Int(c)) => Some(*c as u32),
            _ => None,
        })
    })
}

/// `LayerBipedArmor` input for an armor item (ids 298-317), else `None`.
pub fn armor_piece(cat: &Catalogue, stack: &ItemStack) -> Option<mc_render::avatar::ArmorPiece> {
    let id = cat.items[stack.item].id;
    if !(298..=317).contains(&id) {
        return None;
    }
    // Item order leather, chain, iron, diamond, gold; ArmorMaterial order swaps the last two.
    let material = [0, 1, 2, 4, 3][(id as usize - 298) / 4];
    // ItemArmor.getColor
    let color = if material != 0 {
        0xFFFFFF
    } else {
        stack
            .tag
            .and_then(|t| compound(&t.get(), "display").and_then(|d| match d.get("color") {
                Some(Tag::Int(c)) => Some(*c as u32),
                _ => None,
            }))
            .unwrap_or(10511680)
    };
    Some(mc_render::avatar::ArmorPiece { material, color, glint: is_enchanted(stack) })
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
    let named = has_display_name(stack);
    let mut s = if def.id == 373 && !named { potion_name(lang, stack) } else { display_name(cat, stack) };
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

/// `Item.addInformation` for the items that override it.
fn add_information(lang: &Lang, id: u16, stack: &ItemStack, tag: Option<&Compound>, list: &mut Vec<String>) {
    match id {
        373 => potion_information(lang, stack, list),
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
