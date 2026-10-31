package rustmc.forge;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.mojang.authlib.GameProfile;
import java.lang.reflect.Field;
import java.util.ArrayList;
import java.util.Collection;
import java.util.Collections;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.UUID;
import java.util.WeakHashMap;
import net.minecraft.client.entity.AbstractClientPlayer;
import net.minecraft.client.model.ModelPlayer;
import net.minecraft.client.renderer.entity.RenderItem;
import net.minecraft.client.renderer.entity.RenderManager;
import net.minecraft.client.renderer.entity.RenderPlayer;
import net.minecraft.entity.DataWatcher;
import net.minecraft.entity.Entity;
import net.minecraft.entity.EntityLivingBase;
import net.minecraft.entity.ai.attributes.IAttribute;
import net.minecraft.entity.item.EntityItem;
import net.minecraft.entity.player.EntityPlayer;
import net.minecraft.entity.player.EnumPlayerModelParts;
import net.minecraft.entity.player.InventoryPlayer;
import net.minecraft.entity.player.PlayerCapabilities;
import net.minecraft.item.ItemStack;
import net.minecraft.potion.Potion;
import net.minecraft.potion.PotionEffect;
import net.minecraft.util.BlockPos;
import net.minecraft.util.ChatComponentText;
import net.minecraft.util.FoodStats;
import net.minecraft.util.ResourceLocation;
import net.minecraft.world.World;

/**
 * Entity / EntityLivingBase / EntityPlayer / AbstractClientPlayer methods, answered from the stub fields
 * `Live.sync` mirrors plus the few values no vanilla field holds (`Live` keeps those in {@link #EXTRA}).
 * Fake entities mods build with `new` get sane defaults; see {@link #construct}.
 */
public final class Entities {
    /** Returned by {@link #invoke} for a key this class does not implement, so `Live` can answer it. */
    public static final Object UNHANDLED = new Object();
    // float slots of the per-entity mirror; the last four are only interesting for the local player.
    private static final int BLOCK = 0, ARMOR = 1, ARROWS = 2, SKIN = 3, ABSORB = 4, SWING = 5, PREV_SWING = 6, GAME = 7;
    private static final int SLOTS = 8;
    private static final Map<Entity, float[]> EXTRA = Collections.synchronizedMap(new WeakHashMap<Entity, float[]>());
    private static final Map<Class<?>, Map<String, Field>> FIELDS = new HashMap<Class<?>, Map<String, Field>>();
    private static final ResourceLocation DEFAULT_SKIN = new ResourceLocation("textures/entity/steve.png");
    private static final Map<RenderPlayer, ModelPlayer> MODELS = new WeakHashMap<RenderPlayer, ModelPlayer>();
    private static Map<String, RenderPlayer> SKIN_MAP;
    /** The server's game mode for our player (0 survival .. 3 spectator), -1 before the first sync. */
    static int gameMode = -1;
    private static int nextId = 100000;
    /** Local-player state that arrived before its entity existed (its first sync tick). */
    private static JsonObject pending;

    private Entities() {}

    // ---- reflection over stub fields -------------------------------------------------------------------

    private static Field field(Class<?> type, String name) {
        for (Class<?> c = type; c != null; c = c.getSuperclass()) {
            Map<String, Field> cache = FIELDS.computeIfAbsent(c, k -> new HashMap<String, Field>());
            if (cache.containsKey(name)) {
                Field f = cache.get(name);
                if (f != null) return f;
                continue;
            }
            Field f = null;
            try {
                f = c.getDeclaredField(name);
                f.setAccessible(true);
            } catch (Exception ignored) {}
            cache.put(name, f);
            if (f != null) return f;
        }
        return null;
    }

    private static Object get(Object o, String name) {
        if (o == null) return null;
        Field f = field(o.getClass(), name);
        try {
            return f == null ? null : f.get(o);
        } catch (Exception ignored) {
            return null;
        }
    }

    private static void set(Object o, String name, Object value) {
        if (o == null) return;
        Field f = field(o.getClass(), name);
        if (f == null) return;
        try {
            f.set(o, value);
        } catch (Exception ignored) {}
    }

    private static double number(Object o, String name) {
        Object v = get(o, name);
        return v instanceof Number ? ((Number) v).doubleValue() : 0.0;
    }

    /** The first `ItemStack`-typed field, for classes whose field name differs from the one we know. */
    private static ItemStack stackField(Object o) {
        if (o == null) return null;
        for (Class<?> c = o.getClass(); c != null && c != Object.class; c = c.getSuperclass()) {
            for (Field f : c.getDeclaredFields()) {
                if (f.getType() != ItemStack.class) continue;
                try {
                    f.setAccessible(true);
                    return (ItemStack) f.get(o);
                } catch (Exception ignored) {}
            }
        }
        return null;
    }

    /** `InventoryPlayer.armorInventory`: the four-long `ItemStack[]` of the inventory. */
    private static ItemStack[] armorArray(Object inventory) {
        if (inventory == null) return null;
        for (Class<?> c = inventory.getClass(); c != null && c != Object.class; c = c.getSuperclass()) {
            for (Field f : c.getDeclaredFields()) {
                if (f.getType() != ItemStack[].class) continue;
                try {
                    f.setAccessible(true);
                    ItemStack[] a = (ItemStack[]) f.get(inventory);
                    if (a != null && a.length == 4) return a;
                } catch (Exception ignored) {}
            }
        }
        return null;
    }

    // ---- snapshot ---------------------------------------------------------------------------------------

    private static float extra(Entity e, int slot) {
        float[] v = EXTRA.get(e);
        return v == null ? 0.0F : v[slot];
    }

    private static void put(Entity e, int slot, float value) {
        float[] v = EXTRA.get(e);
        if (v == null) {
            v = new float[SLOTS];
            EXTRA.put(e, v);
        }
        v[slot] = value;
    }

    private static int integer(JsonObject o, String key) {
        JsonElement v = o.get(key);
        return v == null || v.isJsonNull() ? 0 : v.getAsInt();
    }

    private static float floats(JsonObject o, String key) {
        JsonElement v = o.get(key);
        return v == null || v.isJsonNull() ? 0.0F : v.getAsFloat();
    }

    /**
     * Applies the local player's state (the snapshot's `state` member) to the Java entity. Values that no
     * vanilla field holds go to {@link #EXTRA}; `Live.sync` calls this before the entity loop, so state for an
     * entity created by that same tick is retried on the next one instead of being lost.
     */
    static void state(JsonObject root) {
        try {
            if (pending != null) {
                Entity e = Live.entity(integer(pending, "id"));
                if (e != null) {
                    apply(e, pending);
                    pending = null;
                }
            }
            JsonElement state = root.get("state");
            if (state == null || !state.isJsonObject()) return;
            JsonObject o = state.getAsJsonObject();
            Entity e = Live.entity(integer(o, "id"));
            if (e == null) {
                pending = o;
                return;
            }
            apply(e, o);
        } catch (Exception ignored) {}
    }

    private static void apply(Entity e, JsonObject o) throws Exception {
        gameMode = integer(o, "game");
        put(e, BLOCK, floats(o, "block"));
        put(e, ARMOR, floats(o, "armor"));
        put(e, ARROWS, floats(o, "arrows"));
        put(e, SKIN, floats(o, "skin"));
        put(e, ABSORB, floats(o, "absorb"));
        put(e, SWING, floats(o, "swing"));
        put(e, PREV_SWING, floats(o, "prev_swing"));
        put(e, GAME, gameMode);
        // The fields the stubs' own methods read, so Java code reaching them directly sees the same values.
        set(e, "field_110151_bq", Float.valueOf(floats(o, "absorb")));
        set(e, "field_70733_aJ", Float.valueOf(floats(o, "swing")));
        set(e, "field_70732_aI", Float.valueOf(floats(o, "prev_swing")));
        set(e, "field_110158_av", Integer.valueOf(integer(o, "swing_int")));
        set(e, "field_70172_ad", Integer.valueOf(integer(o, "hurt_resist")));
        set(e, "field_70720_be", Integer.valueOf(integer(o, "arrows")));
        JsonElement effects = o.get("effects");
        if (effects != null && effects.isJsonArray() && e instanceof EntityLivingBase) {
            Map<Integer, PotionEffect> map = new HashMap<Integer, PotionEffect>();
            for (JsonElement row : effects.getAsJsonArray()) {
                JsonArray a = row.getAsJsonArray();
                PotionEffect fx = new PotionEffect(a.get(0).getAsInt(), a.get(1).getAsInt(), a.get(2).getAsInt());
                map.put(Integer.valueOf(fx.func_76456_a()), fx);
            }
            set(e, "field_70713_bf", map);
        }
        if (!(e instanceof EntityPlayer)) return;
        JsonElement using = o.get("using");
        set(e, "field_71074_e", using == null || using.isJsonNull() ? null : Live.stack(using, (ItemStack) get(e, "field_71074_e")));
        set(e, "field_71072_f", Integer.valueOf(integer(o, "use_count")));
        FoodStats food = (FoodStats) get(e, "field_71100_bB");
        if (food == null) {
            food = new FoodStats();
            set(e, "field_71100_bB", food);
        }
        food.func_75122_a(integer(o, "food") - food.func_75116_a(), floats(o, "sat") - food.func_75115_e());
        ItemStack[] armor = armorArray(get(e, "field_71071_by"));
        JsonElement items = o.get("armor_items");
        if (armor != null && items != null && items.isJsonArray()) {
            JsonArray a = items.getAsJsonArray();
            for (int i = 0; i < armor.length && i < a.size(); ++i)
                armor[i] = a.get(i).isJsonNull() ? null : Live.stack(a.get(i), armor[i]);
        }
        PlayerCapabilities caps = (PlayerCapabilities) get(e, "field_71075_bZ");
        if (caps != null && gameMode >= 0) set(caps, "field_75098_d", Boolean.valueOf(gameMode == 1));
    }

    // ---- constructors mods call to build fake entities --------------------------------------------------

    /**
     * `Entity`'s constructor work for a mod-allocated object: the stub constructors only dispatch here, so
     * world, entity id, data watcher and (for players) profile/inventory/food must be pushed by hand to keep
     * the object usable.
     */
    private static void construct(Entity e, Object[] args) throws Exception {
        Object world = args.length > 0 ? args[0] : null;
        set(e, "field_70170_p", world instanceof World ? world : null);
        set(e, "field_145783_c", Integer.valueOf(nextId++));
        set(e, "field_70173_aa", Integer.valueOf(0));
        DataWatcher watcher = new DataWatcher(e);
        set(e, "field_70180_af", watcher);
        watcher.func_75682_a(Integer.valueOf(0), Byte.valueOf((byte) 0));
        if (args.length > 3 && args[1] instanceof Number && args[2] instanceof Number && args[3] instanceof Number) {
            // EntityLightningBolt(world, x, y, z): a positioned, immobile effect entity.
            double x = ((Number) args[1]).doubleValue(), y = ((Number) args[2]).doubleValue(), z = ((Number) args[3]).doubleValue();
            set(e, "field_70165_t", Double.valueOf(x));
            set(e, "field_70163_u", Double.valueOf(y));
            set(e, "field_70161_v", Double.valueOf(z));
            set(e, "field_70169_q", Double.valueOf(x));
            set(e, "field_70167_r", Double.valueOf(y));
            set(e, "field_70166_s", Double.valueOf(z));
            set(e, "field_70130_N", Float.valueOf(0.0F));
            set(e, "field_70131_O", Float.valueOf(0.0F));
        }
        if (!(e instanceof EntityLivingBase)) return;
        watcher.func_75682_a(Integer.valueOf(6), Float.valueOf(20.0F));
        set(e, "field_70713_bf", new HashMap<Integer, PotionEffect>());
        set(e, "field_110151_bq", Float.valueOf(0.0F));
        if (!(e instanceof EntityPlayer)) return;
        watcher.func_75682_a(Integer.valueOf(10), Byte.valueOf((byte) 0));
        watcher.func_75682_a(Integer.valueOf(17), Float.valueOf(0.0F));
        watcher.func_75682_a(Integer.valueOf(18), Integer.valueOf(0));
        EntityPlayer player = (EntityPlayer) e;
        set(e, "field_146106_i", args.length > 1 && args[1] instanceof GameProfile ? args[1] : new GameProfile(UUID.randomUUID(), "Player"));
        set(e, "field_71071_by", new InventoryPlayer(player));
        set(e, "field_71075_bZ", new PlayerCapabilities());
        set(e, "field_71100_bB", new FoodStats());
        set(e, "field_70130_N", Float.valueOf(0.6F));
        set(e, "field_70131_O", Float.valueOf(1.8F));
    }

    // ---- dispatch ---------------------------------------------------------------------------------------

    public static Object invoke(String key, Object self, Object[] args) throws Exception {
        String method = key.substring(key.lastIndexOf('.') + 1);
        if (method.equals("<init>")) {
            if (self instanceof Entity) construct((Entity) self, args);
            return null;
        }
        if (self instanceof RenderItem) // drawing item icons needs the GUI renderer, which Rust owns
            return method.equals("func_175050_a") ? Boolean.FALSE : null;
        if (self instanceof RenderManager) {
            if (method.equals("getSkinMap")) return skinMap((RenderManager) self);
            if (method.equals("func_147940_a")) return Boolean.FALSE;
            return null; // setPlayerViewY / setRenderShadow only feed the GL renderer
        }
        if (self instanceof RenderPlayer) return mainModel((RenderPlayer) self);
        if (self instanceof InventoryPlayer || self instanceof EntityItem) return stackField(self);
        if (!(self instanceof Entity)) return UNHANDLED;
        Entity e = (Entity) self;
        if (method.equals("func_70115_ae")) return Boolean.valueOf(get(e, "field_70154_o") != null); // isRiding
        if (method.equals("func_70632_aY")) return Boolean.valueOf(extra(e, BLOCK) != 0.0F);
        if (method.equals("func_71011_bu")) return get(e, "field_71074_e");
        if (method.equals("func_71052_bv")) return Integer.valueOf((int) number(e, "field_71072_f"));
        // getItemInUseDuration: itemInUseCount - getMaxItemUseDuration(); the item's use duration is not
        // modelled here, so the count itself is what a renderer animating the use sees.
        if (method.equals("func_71057_bx")) return Integer.valueOf(get(e, "field_71074_e") == null ? 0 : (int) number(e, "field_71072_f"));
        if (method.equals("func_180425_c"))
            return new BlockPos((int) Math.floor(number(e, "field_70165_t")), (int) Math.floor(number(e, "field_70163_u") + 0.5),
                (int) Math.floor(number(e, "field_70161_v")));
        if (method.equals("func_110306_p")) return DEFAULT_SKIN;
        if (method.equals("func_110303_q")) return null; // no capes: nothing downloads player textures
        if (method.equals("func_175154_l")) return "default";
        if (method.equals("func_175148_a") && args.length > 0 && args[0] instanceof EnumPlayerModelParts) {
            int mask = ((EnumPlayerModelParts) args[0]).func_179327_a();
            if (mask == 0) mask = 1 << ((Enum<?>) args[0]).ordinal();
            int layers = (int) extra(e, SKIN);
            return Boolean.valueOf((layers & mask) == mask);
        }
        if (method.equals("func_110139_bj")) return Float.valueOf(extra(e, ABSORB));
        if (method.equals("func_180799_ab")) return Boolean.FALSE;
        if (method.equals("func_70651_bq")) {
            Collection<PotionEffect> active = potionEffects(e);
            return active == null ? Collections.<PotionEffect>emptyList() : active;
        }
        if (method.equals("func_70660_b") && args.length > 0 && args[0] instanceof Potion) {
            Map<Integer, PotionEffect> map = potionMap(e);
            return map == null ? null : map.get(Integer.valueOf(((Potion) args[0]).func_76396_c()));
        }
        if (method.equals("func_85030_a")) return null; // playSound: the client's sound engine is not modelled
        if (method.equals("func_110148_a")) return null; // entity attributes are not modelled yet
        if (method.equals("func_70658_aO")) return Integer.valueOf((int) extra(e, ARMOR));
        if (method.equals("func_70678_g") && args.length > 0) {
            float prev = extra(e, PREV_SWING), f = extra(e, SWING) - prev;
            if (f < 0.0F) ++f;
            return Float.valueOf(prev + f * ((Number) args[0]).floatValue());
        }
        if (method.equals("func_71024_bL")) return get(e, "field_71100_bB");
        if (method.equals("func_82169_q") && args.length > 0) {
            ItemStack[] armor = armorArray(get(e, "field_71071_by"));
            int slot = ((Number) args[0]).intValue();
            return armor == null || slot < 0 || slot >= armor.length ? null : armor[slot];
        }
        if (method.equals("func_174818_b") && args.length > 0 && args[0] instanceof BlockPos) {
            BlockPos pos = (BlockPos) args[0];
            double dx = number(e, "field_70165_t") - (pos.func_177958_n() + 0.5);
            double dy = number(e, "field_70163_u") - (pos.func_177956_o() + 0.5);
            double dz = number(e, "field_70161_v") - (pos.func_177952_p() + 0.5);
            return Double.valueOf(dx * dx + dy * dy + dz * dz);
        }
        if (method.equals("func_70057_ab")) { // performHurtAnimation
            set(e, "field_70737_aN", Integer.valueOf(10));
            set(e, "field_70738_aO", Integer.valueOf(10));
            set(e, "field_70739_aP", Float.valueOf(0.0F));
            return null;
        }
        if (method.equals("func_110144_aD")) return get(e, "field_110150_bn");
        if (method.equals("func_142013_aG")) return Integer.valueOf((int) number(e, "field_142016_bo"));
        if (method.equals("func_145748_c_")) {
            String name = e.func_70005_c_();
            return new ChatComponentText(name == null ? "" : name);
        }
        if (method.equals("func_70631_g_") || method.equals("func_70608_bn")) return Boolean.FALSE;
        if (method.equals("func_98034_c")) {
            if (!e.func_82150_aj()) return Boolean.FALSE;
            if (args.length > 0 && args[0] instanceof EntityPlayer && ((EntityPlayer) args[0]).func_175149_v()) return Boolean.FALSE;
            return Boolean.TRUE;
        }
        if (method.equals("func_175149_v")) return Boolean.valueOf(extra(e, GAME) == 3.0F);
        if (method.equals("func_110261_ca")) return Boolean.FALSE; // horses are not synced with their data watcher
        if (method.equals("func_177087_b")) return null;
        return UNHANDLED;
    }

    @SuppressWarnings("unchecked")
    private static Map<Integer, PotionEffect> potionMap(Entity e) {
        Object map = get(e, "field_70713_bf");
        return map instanceof Map ? (Map<Integer, PotionEffect>) map : null;
    }

    private static Collection<PotionEffect> potionEffects(Entity e) {
        Map<Integer, PotionEffect> map = potionMap(e);
        return map == null ? null : map.values();
    }

    private static synchronized Map<String, RenderPlayer> skinMap(RenderManager manager) {
        if (SKIN_MAP == null) {
            Map<String, RenderPlayer> map = new LinkedHashMap<String, RenderPlayer>();
            map.put("default", new RenderPlayer(manager, false));
            map.put("slim", new RenderPlayer(manager, true));
            SKIN_MAP = map;
        }
        return SKIN_MAP;
    }

    /** `RenderPlayer.getMainModel`: a shared `ModelPlayer` so mods can hide/extend the player layers. */
    private static synchronized ModelPlayer mainModel(RenderPlayer renderer) {
        ModelPlayer model = MODELS.get(renderer);
        if (model == null) {
            model = new ModelPlayer(0.0F, false);
            set(renderer, "field_177098_i", model);
            MODELS.put(renderer, model);
        }
        return model;
    }
}
