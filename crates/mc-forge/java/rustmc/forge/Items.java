package rustmc.forge;

import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import net.minecraft.client.resources.I18n;
import net.minecraft.entity.Entity;
import net.minecraft.inventory.IInventory;
import net.minecraft.item.Item;
import net.minecraft.item.ItemArmor;
import net.minecraft.item.ItemStack;
import net.minecraft.nbt.NBTTagCompound;
import net.minecraft.nbt.NBTTagList;
import net.minecraft.potion.PotionEffect;
import net.minecraft.potion.PotionHelper;

/**
 * The item API mods read: everything that lives in Java state (ItemStack NBT, slot/container fields, tooltips).
 * `Item` objects are allocated per id by `Live` without their constructors, so all item data comes from Stub
 * calls into Rust (`func_77658_a`, `func_77612_l`, `func_77645_m`, `func_150905_g`, ...).
 */
public final class Items {
    private static final String ITALIC = "\u00a7o";
    private static final String RESET = "\u00a7r";
    private static final String GRAY = "\u00a77";
    private static final String DARK_GRAY = "\u00a78";
    private static final String BLUE = "\u00a79";

    /** Registry id of a live `Item` (-1 when it is not one); Rust owns the registry, this reads `Live`'s map. */
    public static int itemId(Object item) {
        if (!(item instanceof Item)) return -1;
        return Item.func_150891_b((Item) item);
    }

    /** The `ItemArmor$ArmorMaterial` constant of that name: Rust keeps the material an item was built with.
     *  The enum is its own class file here, so it is loaded by name rather than through the outer class. */
    public static Object material(String name) {
        try {
            for (Object value : Class.forName("net.minecraft.item.ItemArmor$ArmorMaterial").getEnumConstants()) {
                if (((Enum<?>) value).name().equals(name)) return value;
            }
        } catch (ClassNotFoundException e) {
            return null;
        }
        return null;
    }

    public static Object invoke(String key, Object self, Object[] args) {
        String method = key.substring(key.lastIndexOf('.') + 1);
        switch (method) {
            case "func_82833_r": return displayName((ItemStack) self);
            case "func_77942_o": return hasTag(self);
            case "func_77948_v": return isItemEnchanted(self);
            case "func_77986_q": return enchantmentTagList(self);
            case "func_77984_f": return isDamageable(self);
            case "func_77951_h": return isDamageable(self) && damage(self) > 0;
            case "func_77962_s": return hasEffect(self);
            case "func_77989_b": return args[0] == null && args[1] == null
                    ? Boolean.TRUE : Boolean.valueOf(args[0] != null && args[1] != null
                            && sameStack((ItemStack) args[0], (ItemStack) args[1]));
            case "func_82840_a": return tooltip((ItemStack) self, (Entity) args[0], ((Boolean) args[1]).booleanValue());
            case "func_82790_a": return Integer.valueOf(tint((ItemStack) args[0], ((Integer) args[1]).intValue()));
            case "func_82814_b": return Integer.valueOf(armorColor((ItemStack) args[0]));
            case "func_77832_l": return potionEffects((ItemStack) args[0]);
            case "func_75211_c": return slotStack(self);
            case "func_75216_d": return Boolean.valueOf(slotStack(self) != null);
            case "func_75139_a": return containerSlot(self, ((Integer) args[0]).intValue());
            case "func_85151_d": return lowerChest(self);
            case "func_70445_o": return heldStack(self);
            case "func_77506_a": return Integer.valueOf(enchantmentLevel(((Integer) args[0]).intValue(), (ItemStack) args[1]));
            case "func_82781_a": return enchantments((ItemStack) args[0]);
            case "func_180319_a": return Integer.valueOf(respiration((Entity) args[0]));
            default: return null;
            case "func_77958_k": return Integer.valueOf(maxDamage((ItemStack) self));
            case "func_77988_m": return Integer.valueOf(useDuration((ItemStack) self));
            case "func_77975_n": return useAction((ItemStack) self);
        }
    }

    // ---- ItemStack NBT ----

    private static boolean hasTag(Object stack) {
        return ((ItemStack) stack).func_77978_p() != null;
    }

    private static boolean isItemEnchanted(Object stack) {
        NBTTagCompound tag = ((ItemStack) stack).func_77978_p();
        return tag != null && tag.func_150297_b("ench", 9);
    }

    private static NBTTagList enchantmentTagList(Object stack) {
        NBTTagCompound tag = ((ItemStack) stack).func_77978_p();
        return tag != null ? tag.func_150295_c("ench", 10) : null;
    }

    private static boolean isDamageable(Object stack) {
        ItemStack s = (ItemStack) stack;
        Item item = s.func_77973_b();
        if (item == null || item.func_77612_l() <= 0) return false;
        NBTTagCompound tag = s.func_77978_p();
        return tag == null || !tag.func_74767_n("Unbreakable");
    }

    private static int damage(Object stack) {
        return ((ItemStack) stack).func_77952_i();
    }

    private static int maxDamage(ItemStack stack) {
        Item item = stack.func_77973_b();
        return item == null ? 0 : item.func_77612_l();
    }

    /** `Item.getMaxItemUseDuration`: food and potions are eaten or drunk, swung weapons and bows are held. */
    private static int useDuration(ItemStack stack) {
        Item item = stack.func_77973_b();
        if (item == null) return 0;
        String cls = item.getClass().getSimpleName();
        if ("ItemPotion".equals(cls) || "ItemFood".equals(cls) || "ItemBucketMilk".equals(cls)) return 32;
        if ("ItemSword".equals(cls) || "ItemBow".equals(cls)) return 72000;
        return 0;
    }

    private static Object useAction(ItemStack stack) {
        Item item = stack.func_77973_b();
        String cls = item == null ? "" : item.getClass().getSimpleName();
        if ("ItemFood".equals(cls)) return net.minecraft.item.EnumAction.EAT;
        if ("ItemPotion".equals(cls) || "ItemBucketMilk".equals(cls)) return net.minecraft.item.EnumAction.DRINK;
        if ("ItemSword".equals(cls)) return net.minecraft.item.EnumAction.BLOCK;
        if ("ItemBow".equals(cls)) return net.minecraft.item.EnumAction.BOW;
        return net.minecraft.item.EnumAction.NONE;
    }

    private static boolean hasEffect(Object stack) {
        ItemStack s = (ItemStack) stack;
        Item item = s.func_77973_b();
        if (item == null) return false;
        String cls = item.getClass().getSimpleName();
        if ("ItemAppleGold".equals(cls)) return s.func_77960_j() > 0 || isItemEnchanted(s);
        if ("ItemEnchantedBook".equals(cls)) return true;
        return isItemEnchanted(s);
    }

    private static boolean sameStack(ItemStack a, ItemStack b) {
        if (a.func_77976_d() != b.func_77976_d()) return false;
        if (a.func_77973_b() != b.func_77973_b()) return false;
        if (a.func_77960_j() != b.func_77960_j()) return false;
        NBTTagCompound ta = a.func_77978_p();
        NBTTagCompound tb = b.func_77978_p();
        if (ta == null) return tb == null;
        return ta.equals(tb);
    }

    // ---- Item subclasses: half NBT, half Rust ----

    private static int tint(ItemStack stack, int pass) {
        Item item = stack.func_77973_b();
        if (item == null) return 16777215;
        String cls = item.getClass().getSimpleName();
        if ("ItemPotion".equals(cls)) return potionColor(stack.func_77960_j());
        if ("ItemArmor".equals(cls)) return armorColor(stack);
        return 16777215;
    }

    /** `PotionHelper.getLiquidColor(meta, false)`, found by signature: the SRG name is not readable here. */
    private static int potionColor(int meta) {
        for (Method m : PotionHelper.class.getDeclaredMethods()) {
            Class<?>[] p = m.getParameterTypes();
            if (p.length != 2 || p[0] != int.class || p[1] != boolean.class || m.getReturnType() != int.class) continue;
            if (!java.lang.reflect.Modifier.isStatic(m.getModifiers())) continue;
            m.setAccessible(true);
            try {
                return ((Integer) m.invoke(null, Integer.valueOf(meta), Boolean.FALSE)).intValue();
            } catch (ReflectiveOperationException e) {
                return 16777215;
            }
        }
        return 16777215;
    }

    private static int armorColor(ItemStack stack) {
        if (!"LEATHER".equals(armorMaterialName(stack))) return 16777215;
        NBTTagCompound tag = stack.func_77978_p();
        if (tag == null) return 10511680;
        NBTTagCompound display = tag.func_74775_l("display");
        if (display == null || !display.func_150297_b("color", 3)) return 10511680;
        int color = nbtInt(display, "color");
        return color < 0 ? 10511680 : color;
    }

    /** The material name Rust knows for this item, through the stub `getArmorMaterial` it answers. */
    private static String armorMaterialName(ItemStack stack) {
        Item item = stack.func_77973_b();
        if (item == null) return "";
        for (Method m : item.getClass().getMethods()) {
            if (m.getParameterCount() != 0 || !m.getReturnType().getName().endsWith("ArmorMaterial")) continue;
            try {
                Object value = m.invoke(item);
                return value == null ? "" : ((Enum<?>) value).name();
            } catch (ReflectiveOperationException e) {
                return "";
            }
        }
        return "";
    }

    private static int nbtInt(NBTTagCompound tag, String key) {
        for (Method m : NBTTagCompound.class.getDeclaredMethods()) {
            Class<?>[] p = m.getParameterTypes();
            if (p.length != 1 || p[0] != String.class || m.getReturnType() != int.class) continue;
            m.setAccessible(true);
            try {
                return ((Integer) m.invoke(tag, key)).intValue();
            } catch (ReflectiveOperationException e) {
                return -1;
            }
        }
        return -1;
    }

    private static List<PotionEffect> potionEffects(ItemStack stack) {
        List<PotionEffect> out = new ArrayList<PotionEffect>();
        NBTTagCompound tag = stack.func_77978_p();
        if (tag != null && tag.func_150297_b("CustomPotionEffects", 9)) {
            NBTTagList list = tag.func_150295_c("CustomPotionEffects", 10);
            for (int i = 0; i < list.func_74745_c(); i++) {
                PotionEffect effect = PotionEffect.func_82722_b(list.func_150305_b(i));
                if (effect != null) out.add(effect);
            }
            return out;
        }
        int meta = stack.func_77960_j();
        List<?> brewed = PotionHelper.func_77917_b(meta, (meta & 16384) != 0);
        if (brewed != null) for (Object effect : brewed) out.add((PotionEffect) effect);
        return out;
    }

    // ---- Slots, containers, the player's held stack ----

    private static ItemStack slotStack(Object slot) {
        Object direct = byName(slot, "field_75218_e");
        if (direct instanceof ItemStack) return (ItemStack) direct;
        Object inventory = byName(slot, "field_75224_c");
        if (!(inventory instanceof IInventory)) inventory = byType(slot, IInventory.class);
        Object index = byName(slot, "field_75225_a");
        if (inventory instanceof IInventory && index instanceof Integer) {
            return ((IInventory) inventory).func_70301_a(((Integer) index).intValue());
        }
        return null;
    }

    private static Object containerSlot(Object container, int index) {
        Object slots = byName(container, "field_75151_b");
        if (!(slots instanceof List)) return null;
        List<?> list = (List<?>) slots;
        return index >= 0 && index < list.size() ? list.get(index) : null;
    }

    private static Object lowerChest(Object container) {
        Object inventory = byName(container, "field_75155_e");
        return inventory instanceof IInventory ? inventory : byType(container, IInventory.class);
    }

    private static ItemStack heldStack(Object inventory) {
        Object array = byName(inventory, "field_70462_a");
        Object current = byName(inventory, "field_70461_c");
        if (!(array instanceof Object[]) || !(current instanceof Integer)) return null;
        Object[] stacks = (Object[]) array;
        int i = ((Integer) current).intValue();
        return i >= 0 && i < stacks.length && stacks[i] instanceof ItemStack ? (ItemStack) stacks[i] : null;
    }

    // ---- EnchantmentHelper ----

    private static int enchantmentLevel(int id, ItemStack stack) {
        if (stack == null) return 0;
        NBTTagList list = enchantmentTagList(stack);
        if (list == null) return 0;
        for (int i = 0; i < list.func_74745_c(); i++) {
            NBTTagCompound entry = list.func_150305_b(i);
            if (entry.func_74765_d("id") == id) return entry.func_74765_d("lvl");
        }
        return 0;
    }

    private static Map<Integer, Integer> enchantments(ItemStack stack) {
        Map<Integer, Integer> map = new LinkedHashMap<Integer, Integer>();
        if (stack == null) return map;
        NBTTagList list = enchantmentTagList(stack);
        Item item = stack.func_77973_b();
        if (item != null && "ItemEnchantedBook".equals(item.getClass().getSimpleName())) {
            NBTTagCompound tag = stack.func_77978_p();
            list = tag == null ? null : tag.func_150295_c("StoredEnchantments", 10);
        }
        if (list != null) {
            for (int i = 0; i < list.func_74745_c(); i++) {
                NBTTagCompound entry = list.func_150305_b(i);
                map.put(Integer.valueOf(entry.func_74765_d("id")), Integer.valueOf(entry.func_74765_d("lvl")));
            }
        }
        return map;
    }

    private static int maxEnchantmentLevel(int id, ItemStack[] stacks) {
        int max = 0;
        if (stacks == null) return 0;
        for (ItemStack stack : stacks) {
            int level = enchantmentLevel(id, stack);
            if (level > max) max = level;
        }
        return max;
    }

    /** `EnchantmentHelper.getRespiration`: the best respiration on the entity's armour, via `getInventory`. */
    private static int respiration(Entity entity) {
        if (entity == null) return 0;
        for (Method m : entity.getClass().getMethods()) {
            if (m.getParameterCount() != 0 || m.getReturnType() != ItemStack[].class) continue;
            try {
                return maxEnchantmentLevel(5, (ItemStack[]) m.invoke(entity));
            } catch (ReflectiveOperationException e) {
                return 0;
            }
        }
        return 0;
    }

    // ---- Tooltips ----

    private static List<String> tooltip(ItemStack stack, Entity player, boolean advanced) {
        List<String> lines = new ArrayList<String>();
        String name = displayName(stack);
        if (hasDisplayName(stack)) name = ITALIC + name + RESET;
        lines.add(name);
        if (advanced && stack.func_77973_b() != null) lines.add(DARK_GRAY + stack.func_77973_b().func_77658_a());
        enchantmentTooltip(stack, lines);
        if (isDamageable(stack) && damage(stack) > 0) {
            lines.add(GRAY + translate("item.durability", Integer.toString(maxDamage(stack) - damage(stack)),
                    Integer.toString(maxDamage(stack))));
        }
        NBTTagCompound tag = stack.func_77978_p();
        if (tag != null && tag.func_74767_n("Unbreakable")) lines.add(BLUE + translate("item.unbreakable"));
        Item item = stack.func_77973_b();
        if (item != null) {
            try {
                item.getClass().getMethod("func_77624_a", ItemStack.class, Entity.class, List.class, boolean.class)
                        .invoke(item, stack, player, lines, Boolean.valueOf(advanced));
            } catch (ReflectiveOperationException e) {
                // addInformation is optional here; vanilla items only add their own extra lore.
            }
        }
        return lines;
    }

    private static String displayName(ItemStack stack) {
        if (hasDisplayName(stack)) return stack.func_77978_p().func_74775_l("display").func_74779_i("Name");
        Item item = stack.func_77973_b();
        return item == null ? translate("item.empty") : translate(item.func_77658_a());
    }

    private static boolean hasDisplayName(ItemStack stack) {
        NBTTagCompound tag = stack.func_77978_p();
        if (tag == null || !tag.func_150297_b("display", 10)) return false;
        NBTTagCompound display = tag.func_74775_l("display");
        return display != null && display.func_150297_b("Name", 8) && display.func_74779_i("Name").length() > 0;
    }

    private static void enchantmentTooltip(ItemStack stack, List<String> lines) {
        NBTTagList list = enchantmentTagList(stack);
        if (list == null) return;
        for (int i = 0; i < list.func_74745_c(); i++) {
            NBTTagCompound entry = list.func_150305_b(i);
            int id = entry.func_74765_d("id");
            int level = entry.func_74765_d("lvl");
            String key = enchantmentKey(id);
            if (key == null) continue;
            String line = translate(key);
            if (level != 1 || enchantmentMaxLevel(id) != 1) line = line + " " + translate("enchantment.level." + level);
            lines.add(GRAY + line);
        }
    }

    private static final int[] ENCHANTMENT_IDS = {
        0, 1, 2, 3, 4, 5, 6, 7, 8, 16, 17, 18, 19, 20, 21, 32, 33, 34, 35, 48, 49, 50, 51, 61, 62,
    };
    private static final String[] ENCHANTMENT_KEYS = {
        "enchantment.protect.all", "enchantment.protect.fire", "enchantment.protect.fall",
        "enchantment.protect.explosion", "enchantment.protect.projectile", "enchantment.oxygen",
        "enchantment.waterWorker", "enchantment.thorns", "enchantment.waterWalker", "enchantment.damage.all",
        "enchantment.damage.undead", "enchantment.damage.arthropods", "enchantment.knockback", "enchantment.fire",
        "enchantment.lootBonus", "enchantment.digging", "enchantment.untouching", "enchantment.durability",
        "enchantment.lootBonusDigger", "enchantment.arrowDamage", "enchantment.arrowKnockback",
        "enchantment.arrowFire", "enchantment.arrowInfinite", "enchantment.lootBonusFishing",
        "enchantment.fishingSpeed",
    };
    private static final int[] ENCHANTMENT_MAX = {
        4, 4, 4, 4, 4, 3, 1, 3, 3, 5, 5, 5, 2, 2, 3, 5, 1, 3, 3, 5, 2, 1, 1, 3, 3,
    };

    private static String enchantmentKey(int id) {
        for (int i = 0; i < ENCHANTMENT_IDS.length; i++) if (ENCHANTMENT_IDS[i] == id) return ENCHANTMENT_KEYS[i];
        return null;
    }

    private static int enchantmentMaxLevel(int id) {
        for (int i = 0; i < ENCHANTMENT_IDS.length; i++) if (ENCHANTMENT_IDS[i] == id) return ENCHANTMENT_MAX[i];
        return 1;
    }

    /** `StatCollector.translateToLocal("[key].name")`, falling back to the bare key like vanilla does. */
    private static String translate(String key, String... args) {
        String named = translateKey(key + ".name", args);
        if (named != null) return named;
        String bare = translateKey(key, args);
        return bare != null ? bare : key;
    }

    private static String translateKey(String key, String... args) {
        String value = I18n.func_135052_a(key, (Object[]) args);
        return value == null || value.length() == 0 || value.equals(key) ? null : value;
    }

    // ---- Field access: stub state is SRG-named and not always public ----

    private static Object byName(Object owner, String name) {
        if (owner == null) return null;
        for (Class<?> c = owner.getClass(); c != null; c = c.getSuperclass()) {
            for (Field f : c.getDeclaredFields()) {
                if (!f.getName().equals(name)) continue;
                f.setAccessible(true);
                try {
                    return f.get(owner);
                } catch (ReflectiveOperationException e) {
                    return null;
                }
            }
        }
        return null;
    }

    private static Object byType(Object owner, Class<?> type) {
        if (owner == null) return null;
        for (Class<?> c = owner.getClass(); c != null; c = c.getSuperclass()) {
            for (Field f : c.getDeclaredFields()) {
                if (!type.isAssignableFrom(f.getType())) continue;
                f.setAccessible(true);
                try {
                    return f.get(owner);
                } catch (ReflectiveOperationException e) {
                    return null;
                }
            }
        }
        return null;
    }
}
