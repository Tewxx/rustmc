//! The item API mods read: `Item`/`ItemStack` data, armour/sword/food values and `ItemPotion`'s class switch.
//! `Live` allocates `Item` objects per id without their constructors, so every field is unset: the values come
//! from the item registry here, while the Java state (`ItemStack` NBT, slots, containers) is answered by
//! `rustmc.forge.Items` (routed below like `Tab`).
use crate::{runtime as rt, stubs::{register, Call, Ret}};
use jni::{errors::Result, objects::{JObject, JValue}};
use mc_data::item::FieldValue;

/// A registry row; instance fields are vanilla's, by MCP name.
struct ItemData(&'static mc_data::item::Item);

impl ItemData {
    fn num(&self, key: &str) -> Option<f64> { match self.0.field(key) { Some(FieldValue::Num(n)) => Some(n), _ => None } }
    fn text(&self, key: &str) -> Option<&'static str> { match self.0.field(key) { Some(FieldValue::Str(s)) => Some(s), _ => None } }
    fn int(&self, key: &str) -> Option<i32> { self.num(key).map(|v| v as i32) }
    fn id(&self) -> i32 { self.0.id as i32 }
    fn unlocalized(&self) -> &'static str { self.0.unlocalized }
    fn max_damage(&self) -> i32 { self.0.max_damage as i32 }
    /// `Item.ToolMaterial` of a tool: its constant name (`toString`) and its `damageVsEntity`.
    fn tool(&self) -> Option<(&'static str, f32)> { self.0.tool.as_ref().map(|t| (t.name, t.damage)) }
    /// `Item.isDamageable()`: `maxDamage > 0 && !hasSubtypes`.
    fn is_damageable(&self) -> bool { self.0.is_damageable() }
}

fn item(id: i32) -> Option<ItemData> { u16::try_from(id).ok().and_then(mc_data::item::Item::by_id).map(ItemData) }

fn item_by_name(name: &str) -> Option<ItemData> { mc_data::item::Item::by_name(name.trim()).map(ItemData) }

/// `Item.getItemFromBlock`: the `ItemBlock` built from that block (its unlocalized name is the block's).
fn item_of_block(unlocalized: &str) -> Option<ItemData> {
    mc_data::item::Item::all().iter().find(|d| d.unlocalized == unlocalized && d.is_a("ItemBlock")).map(ItemData)
}

pub(crate) fn register_all() {
    // Registry data, per item id. `*.` too: mods subclass and the subclass keeps the vanilla behaviour.
    for (method, handler) in [
        ("func_77658_a", unlocalized as fn(&mut Call<'_, '_>) -> Result<Ret>),
        ("func_77612_l", max_damage),
        ("func_77645_m", is_damageable),
        ("func_77629_n_", rotate_rendering),
    ] {
        register(&format!("net/minecraft/item/Item.{method}"), handler);
        register(&format!("*.{method}"), handler);
    }

    register("net/minecraft/item/Item.func_111206_d", |c| item_by_id(c));
    // ItemStack.getMaxStackSize -> Item.getItemStackLimit: the registry's maxStackSize.
    register("net/minecraft/item/ItemStack.func_77976_d", |c| {
        let stack = c.env.new_local_ref(&c.this)?;
        let object = c.env.get_field(&stack, "field_151002_e", "Lnet/minecraft/item/Item;")?.l()?;
        if object.is_null() { return Ok(Ret::Int(64)); }
        let id = id_of(c, &object)?;
        Ok(Ret::Int(item(id).map_or(64, |d| d.0.max_stack as i32)))
    });
    register("net/minecraft/item/Item.func_150898_a", |c| item_from_block(c));
    register("net/minecraft/item/ItemPotion.func_77831_g", |c| Ok(Ret::Bool(c.i32(0)? & 16384 != 0)));
    register("net/minecraft/item/ItemSword.func_150931_i", |c| {
        Ok(Ret::Float(data(c)?.and_then(|d| d.tool()).map_or(0.0, |(_, damage)| damage + 4.0)))
    });
    register("net/minecraft/item/ItemSword.func_150932_j", |c| {
        Ok(match data(c)?.and_then(|d| d.tool()) {
            Some((name, _)) => Ret::Str(name.to_owned()),
            None => Ret::Str(String::new()),
        })
    });
    register("net/minecraft/item/ItemFood.func_150905_g", |c| {
        Ok(Ret::Int(data(c)?.and_then(|d| d.int("healAmount")).unwrap_or(0)))
    });
    register("net/minecraft/item/ItemArmor.func_82812_d", |c| {
        let Some(name) = data(c)?.and_then(|d| d.text("material")) else { return Ok(Ret::Null) };
        let name = c.env.new_string(name)?;
        let material = rt::call_static(c.env, "rustmc/forge/Items", "material",
            "(Ljava/lang/String;)Ljava/lang/Object;", &[JValue::Object(&name)])?.l()?;
        Ok(Ret::Obj(c.env.new_global_ref(material)?))
    });

    // Java state: NBT, tooltips, slots/containers and the values that need a Java object (EnumAction, lists).
    for (class, methods) in [
        ("net/minecraft/item/ItemStack", &[
            "func_77942_o", "func_77948_v", "func_77986_q", "func_77984_f", "func_77951_h", "func_77962_s",
            "func_77989_b", "func_82840_a", "func_77958_k", "func_77988_m", "func_77975_n", "func_82833_r",
        ][..]),
        ("net/minecraft/item/Item", &["func_82790_a"]),
        ("net/minecraft/item/ItemArmor", &["func_82814_b"]),
        ("net/minecraft/item/ItemPotion", &["func_77832_l"]),
        ("net/minecraft/inventory/Slot", &["func_75211_c", "func_75216_d"]),
        ("net/minecraft/inventory/Container", &["func_75139_a"]),
        ("net/minecraft/inventory/ContainerChest", &["func_85151_d"]),
        ("net/minecraft/entity/player/InventoryPlayer", &["func_70445_o"]),
        ("net/minecraft/enchantment/EnchantmentHelper", &["func_77506_a", "func_82781_a", "func_180319_a"]),
    ] {
        for &method in methods {
            let route = format!("{class}.{method}");
            let handler = route.clone();
            register(&route, move |c| invoke(c, &handler));
            let any = format!("*.{method}");
            let handler = any.clone();
            register(&any, move |c| invoke(c, &handler));
        }
    }
}

fn invoke(c: &mut Call<'_, '_>, key: &str) -> Result<Ret> {
    let name = c.env.new_string(key)?;
    let this = c.env.new_local_ref(&c.this)?;
    let args = JObject::from(c.env.new_local_ref(c.arguments())?);
    let result = rt::call_static(c.env, "rustmc/forge/Items", "invoke",
        "(Ljava/lang/String;Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;",
        &[JValue::Object(&name), JValue::Object(&this), JValue::Object(&args)])?.l()?;
    Ok(Ret::Obj(c.env.new_global_ref(result)?))
}

/// The registry row of the `Item` a handler was called on (`Live`'s id map knows which one it is).
fn data(c: &mut Call<'_, '_>) -> Result<Option<ItemData>> {
    let receiver = c.env.new_local_ref(&c.this)?;
    Ok(item(id_of(c, &receiver)?))
}

fn id_of(c: &mut Call<'_, '_>, item: &JObject<'_>) -> Result<i32> {
    Ok(rt::call_static(c.env, "rustmc/forge/Items", "itemId", "(Ljava/lang/Object;)I",
        &[JValue::Object(item)])?.i()?)
}

/// `Item.getItemById`, through `Live`'s id-to-item map.
fn item_object(c: &mut Call<'_, '_>, id: i32) -> Result<Ret> {
    let item = rt::call_static(c.env, "net/minecraft/item/Item", "func_150899_d",
        "(I)Lnet/minecraft/item/Item;", &[JValue::Int(id)])?.l()?;
    Ok(Ret::Obj(c.env.new_global_ref(item)?))
}

fn unlocalized(c: &mut Call<'_, '_>) -> Result<Ret> {
    Ok(match data(c)? {
        Some(d) => Ret::Str(d.unlocalized().to_owned()),
        None => Ret::Str(String::new()),
    })
}

fn max_damage(c: &mut Call<'_, '_>) -> Result<Ret> {
    Ok(Ret::Int(data(c)?.map_or(0, |d| d.max_damage())))
}

fn is_damageable(c: &mut Call<'_, '_>) -> Result<Ret> {
    Ok(Ret::Bool(data(c)?.map_or(false, |d| d.is_damageable())))
}

/// `Item.shouldRotateAroundWhenRendering`: only the fishing rod renders turned in the hand.
fn rotate_rendering(c: &mut Call<'_, '_>) -> Result<Ret> {
    Ok(Ret::Bool(data(c)?.map_or(false, |d| d.0.is_a("ItemFishingRod"))))
}

/// `Item.getByNameOrId(String)`: a number is an id, anything else a registry name.
fn item_by_id(c: &mut Call<'_, '_>) -> Result<Ret> {
    let Some(name) = c.string(0)? else { return Ok(Ret::Null) };
    let id = match name.trim().parse::<i32>() {
        Ok(id) => id,
        Err(_) => match item_by_name(&name) {
            Some(d) => d.id(),
            None => return Ok(Ret::Null),
        },
    };
    item_object(c, id)
}

/// `Item.getItemFromBlock(Block)`: match the block's unlocalized name (the stub answers it from the registry).
fn item_from_block(c: &mut Call<'_, '_>) -> Result<Ret> {
    let block = c.obj(0)?;
    if block.is_null() { return Ok(Ret::Null); }
    let name = c.env.call_method(&block, "func_149739_a", "()Ljava/lang/String;", &[])?.l()?;
    if name.is_null() { return Ok(Ret::Null); }
    let name: String = c.env.get_string(&jni::objects::JString::from(name))?.into();
    match item_of_block(&name) {
        Some(d) => item_object(c, d.id()),
        None => Ok(Ret::Null),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_data_matches_vanilla() {
        let helmet = item(298).expect("leather helmet");
        assert_eq!(helmet.max_damage(), 55);
        assert_eq!(helmet.text("material"), Some("LEATHER"));
        assert_eq!(helmet.int("armorType"), Some(0));
        assert!(helmet.0.is_a("ItemArmor"));
        assert!(helmet.is_damageable());

        let sword = item(276).expect("diamond sword");
        assert_eq!(sword.tool(), Some(("EMERALD", 3.0)));
        assert!(!sword.0.has_subtypes && sword.0.full3d);

        assert_eq!(item(320).and_then(|d| d.int("healAmount")), Some(8));
        assert_eq!(item_by_name("minecraft:diamond_sword").map(|d| d.id()), Some(276));
        assert_eq!(item_by_name("diamond_sword").map(|d| d.id()), Some(276));
        assert_eq!(item_of_block("tile.stone").map(|d| d.id()), Some(1));
        assert!(!item(1).expect("stone").is_damageable());
    }
}
