package rustmc.extract;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.lang.reflect.Field;
import java.lang.reflect.Modifier;
import java.util.Collection;
import net.minecraft.block.Block;
import net.minecraft.enchantment.Enchantment;
import net.minecraft.item.Item;
import net.minecraft.item.ItemStack;

/**
 * Item registry: id, name, class chain, and every instance field declared by the item's classes
 * (stack size, durability, tool/armor/food stats...), so behaviour can be dispatched in Rust.
 * Also the enchantment table (`enchantments`) used by loot and the enchanting table.
 */
public final class DumpItems {
    private DumpItems() {}

    public static JsonArray dump() throws Exception {
        JsonArray out = new JsonArray();
        for (Item item : Item.itemRegistry) {
            JsonObject o = new JsonObject();
            o.addProperty("id", Item.getIdFromItem(item));
            o.addProperty("name", Item.itemRegistry.getNameForObject(item).toString());
            o.addProperty("unlocalized", item.getUnlocalizedName());
            o.addProperty("max_stack", item.getItemStackLimit());
            o.addProperty("max_damage", item.getMaxDamage());
            o.addProperty("has_subtypes", item.getHasSubtypes());
            o.addProperty("enchantability", item.getItemEnchantability());
            o.addProperty("full3d", item.isFull3D());
            o.addProperty("container", item.getContainerItem() == null ? "" : Item.itemRegistry.getNameForObject(item.getContainerItem()).toString());
            JsonArray chain = new JsonArray();
            JsonObject fields = new JsonObject();
            for (Class<?> c = item.getClass(); c != null && Item.class.isAssignableFrom(c); c = c.getSuperclass()) {
                chain.add(Json.str(c.getSimpleName()));
                for (Field f : c.getDeclaredFields()) {
                    if (Modifier.isStatic(f.getModifiers())) {
                        continue;
                    }
                    f.setAccessible(true);
                    Object v = f.get(item);
                    String key = f.getName();
                    if (fields.has(key)) {
                        continue;
                    }
                    if (v instanceof Number) {
                        fields.addProperty(key, ((Number) v).doubleValue());
                    } else if (v instanceof Boolean) {
                        fields.addProperty(key, (Boolean) v);
                    } else if (v instanceof String) {
                        fields.addProperty(key, (String) v);
                    } else if (v instanceof Enum) {
                        fields.addProperty(key, ((Enum<?>) v).name());
                    } else if (v instanceof Block) {
                        fields.addProperty(key, Block.blockRegistry.getNameForObject((Block) v).toString());
                    } else if (v instanceof Item) {
                        fields.addProperty(key, Item.itemRegistry.getNameForObject((Item) v).toString());
                    } else if (v instanceof Collection) {
                        JsonArray a = new JsonArray();
                        for (Object e : (Collection<?>) v) {
                            if (e instanceof Block) {
                                a.add(Json.str(Block.blockRegistry.getNameForObject((Block) e).toString()));
                            }
                        }
                        if (a.size() > 0) {
                            fields.add(key, a);
                        }
                    }
                }
            }
            o.add("classes", chain);
            o.add("fields", fields);
            // Tool materials carry their own stats.
            for (Class<?> c = item.getClass(); c != null; c = c.getSuperclass()) {
                for (Field f : c.getDeclaredFields()) {
                    if (f.getType() == Item.ToolMaterial.class && !Modifier.isStatic(f.getModifiers())) {
                        f.setAccessible(true);
                        Item.ToolMaterial m = (Item.ToolMaterial) f.get(item);
                        JsonObject t = new JsonObject();
                        t.addProperty("name", m.name());
                        t.addProperty("harvest_level", m.getHarvestLevel());
                        t.addProperty("max_uses", m.getMaxUses());
                        t.addProperty("efficiency", (double) m.getEfficiencyOnProperMaterial());
                        t.addProperty("damage", (double) m.getDamageVsEntity());
                        t.addProperty("enchantability", m.getEnchantability());
                        o.add("tool_material", t);
                    }
                }
            }
            out.add(o);
        }
        return out;
    }

    public static JsonArray enchantments() {
        JsonArray out = new JsonArray();
        for (Enchantment e : Enchantment.enchantmentsBookList) {
            if (e == null) {
                continue;
            }
            JsonObject o = new JsonObject();
            o.addProperty("id", e.effectId);
            o.addProperty("name", e.getName());
            o.addProperty("class", e.getClass().getSimpleName());
            o.addProperty("weight", e.getWeight());
            o.addProperty("type", e.type.name());
            o.addProperty("min_level", e.getMinLevel());
            o.addProperty("max_level", e.getMaxLevel());
            JsonArray min = new JsonArray();
            JsonArray max = new JsonArray();
            for (int l = 1; l <= e.getMaxLevel(); l++) {
                min.add(Json.num(e.getMinEnchantability(l)));
                max.add(Json.num(e.getMaxEnchantability(l)));
            }
            o.add("min_enchantability", min);
            o.add("max_enchantability", max);
            JsonArray compatible = new JsonArray();
            for (Enchantment f : Enchantment.enchantmentsBookList) {
                if (f != null && e.canApplyTogether(f)) {
                    compatible.add(Json.num(f.effectId));
                }
            }
            o.add("compatible", compatible);
            JsonArray items = new JsonArray();
            for (Item item : Item.itemRegistry) {
                if (e.type.canEnchantItem(item)) {
                    items.add(Json.num(Item.getIdFromItem(item)));
                }
            }
            o.add("table_items", items);
            JsonArray anvil = new JsonArray();
            for (Item item : Item.itemRegistry) {
                if (e.canApply(new ItemStack(item))) {
                    anvil.add(Json.num(Item.getIdFromItem(item)));
                }
            }
            o.add("anvil_items", anvil);
            out.add(o);
        }
        return out;
    }
}
