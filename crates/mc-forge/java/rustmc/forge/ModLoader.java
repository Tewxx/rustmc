package rustmc.forge;

import java.io.File;
import java.lang.reflect.Constructor;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.util.ArrayList;
import java.util.Enumeration;
import java.util.List;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;
import net.minecraftforge.fml.common.Mod;
import net.minecraftforge.fml.common.ModContainer;
import net.minecraftforge.fml.common.ModMetadata;
import net.minecraftforge.fml.common.event.FMLInitializationEvent;
import net.minecraftforge.fml.common.event.FMLPostInitializationEvent;
import net.minecraftforge.fml.common.event.FMLPreInitializationEvent;

/** Finds @Mod classes in jars already on the class path and runs their pre-init, init and post-init handlers. */
public final class ModLoader {
    private static final File CONFIG = new File("config");

    private static boolean registered;
    /** The containers of the mods that ran (Loader.getActiveModList); see Fml for the pre-load fallback. */
    static final List<Object> LOADED = new ArrayList<>();

    /** Bootstrap.register's registry half: blocks, fire spread, then items (items need their blocks). */
    static void registries() {
        if (registered) return;
        registered = true;
        try {
            net.minecraft.block.Block.func_149671_p();
            net.minecraft.block.BlockFire.func_149843_e();
            net.minecraft.item.Item.func_150900_l();
            entityList();
        } catch (Throwable t) {
            System.err.println("[forge] vanilla registries failed to build");
            t.printStackTrace();
        }
    }

    /**
     * EntityList is stubbed (its static initializer needs StatList), so its name/class maps are null and mods that read
     * them reflectively (LunarForge's mob-size module) crash. Fill the two maps with vanilla's mob registrations.
     */
    private static void entityList() throws Exception {
        String[][] mobs = {
            {"Creeper", "monster.EntityCreeper"}, {"Skeleton", "monster.EntitySkeleton"}, {"Spider", "monster.EntitySpider"},
            {"Giant", "monster.EntityGiantZombie"}, {"Zombie", "monster.EntityZombie"}, {"Slime", "monster.EntitySlime"},
            {"Ghast", "monster.EntityGhast"}, {"PigZombie", "monster.EntityPigZombie"}, {"Enderman", "monster.EntityEnderman"},
            {"CaveSpider", "monster.EntityCaveSpider"}, {"Silverfish", "monster.EntitySilverfish"}, {"Blaze", "monster.EntityBlaze"},
            {"LavaSlime", "monster.EntityMagmaCube"}, {"EnderDragon", "boss.EntityDragon"}, {"WitherBoss", "boss.EntityWither"},
            {"Bat", "passive.EntityBat"}, {"Witch", "monster.EntityWitch"}, {"Endermite", "monster.EntityEndermite"},
            {"Guardian", "monster.EntityGuardian"}, {"Pig", "passive.EntityPig"}, {"Sheep", "passive.EntitySheep"},
            {"Cow", "passive.EntityCow"}, {"Chicken", "passive.EntityChicken"}, {"Squid", "passive.EntitySquid"},
            {"Wolf", "passive.EntityWolf"}, {"MushroomCow", "passive.EntityMooshroom"}, {"SnowMan", "monster.EntitySnowman"},
            {"Ozelot", "passive.EntityOcelot"}, {"VillagerGolem", "monster.EntityIronGolem"}, {"EntityHorse", "passive.EntityHorse"},
            {"Rabbit", "passive.EntityRabbit"}, {"Villager", "passive.EntityVillager"}};
        java.util.Map<String, Class<?>> byName = new java.util.HashMap<>();
        java.util.Map<Class<?>, String> byClass = new java.util.HashMap<>();
        ClassLoader loader = ModLoader.class.getClassLoader();
        for (String[] m : mobs) {
            Class<?> c = Class.forName("net.minecraft.entity." + m[1], false, loader);
            byName.put(m[0], c);
            byClass.put(c, m[0]);
        }
        Class<?> list = Class.forName("net.minecraft.entity.EntityList", false, loader);
        for (String[] f : new String[][] {{"field_75625_b", "name"}, {"field_75626_c", "class"}}) {
            java.lang.reflect.Field field = list.getDeclaredField(f[0]);
            field.setAccessible(true);
            java.lang.reflect.Field modifiers = java.lang.reflect.Field.class.getDeclaredField("modifiers");
            modifiers.setAccessible(true);
            modifiers.setInt(field, field.getModifiers() & ~java.lang.reflect.Modifier.FINAL);
            if (field.get(null) == null) field.set(null, f[1].equals("name") ? byName : byClass);
        }
    }

    /** Returns the ids of the mods that ran. A mod that throws is reported and skipped; the others still load. */
    public static String[] load(String[] jars) throws Exception {
        List<Object> mods = new ArrayList<>();
        List<String> ids = new ArrayList<>();
        List<Object> containers = new ArrayList<>();
        // Vanilla's GameSettings constructor makes every KeyBinding before any mod runs.
        net.minecraft.client.Minecraft mc = net.minecraft.client.Minecraft.func_71410_x();
        if (mc != null && mc.field_71474_y != null) Live.defaultKeys(mc.field_71474_y);
        registries();
        if (mc != null) { Live.renderManager(mc); Live.timer(mc); }
        for (String jar : jars) {
            try (ZipFile zip = new ZipFile(jar)) {
                for (Enumeration<? extends ZipEntry> e = zip.entries(); e.hasMoreElements();) {
                    String n = e.nextElement().getName();
                    if (!n.endsWith(".class") || n.contains("$")) continue;
                    Class<?> c;
                    try {
                        c = Class.forName(n.substring(0, n.length() - 6).replace('/', '.'), false, ModLoader.class.getClassLoader());
                    } catch (Throwable t) {
                        continue; // e.g. a mixin class: Mixin refuses to load it directly
                    }
                    Mod mod = c.getAnnotation(Mod.class);
                    if (mod == null) continue;
                    try { Object instance = c.newInstance(); mods.add(instance); ids.add(mod.modid()); Object box = container(mod, instance, new File(jar)); containers.add(box); LOADED.add(box); }
                    catch (Throwable t) { System.err.println("[forge] cannot construct mod " + mod.modid()); t.printStackTrace(); }
                }
            }
        }
        // Events share one shape: a varargs Object[] constructor fed (asmData, configDir, configFile / unused).
        Object[] data = { null, CONFIG, new File(CONFIG, "mod.cfg") };
        for (Class<?> phase : new Class<?>[] { FMLPreInitializationEvent.class, FMLInitializationEvent.class, FMLPostInitializationEvent.class }) {
            Constructor<?> k = phase.getConstructor(Object[].class);
            for (int i = 0; i < mods.size(); i++) for (Method m : mods.get(i).getClass().getMethods()) {
                Object mod = mods.get(i);
                if (!m.isAnnotationPresent(Mod.EventHandler.class) || m.getParameterTypes().length != 1 || m.getParameterTypes()[0] != phase) continue;
                try {
                    // FMLModContainer hands each mod its own event with applyModContainer (source, metadata, mod log, config file).
                    Object event = k.newInstance((Object) data);
                    Method apply = phase.getMethod("applyModContainer", ModContainer.class);
                    apply.invoke(event, containers.get(i));
                    m.invoke(mod, event);
                }
                catch (Throwable t) { System.err.println("[forge] " + m + " failed"); t.printStackTrace(); }
            }
        }
        return ids.toArray(new String[0]);
    }

    /** A ModContainer for a mod we loaded ourselves: its @Mod values, metadata, source jar and instance. */
    private static Object container(Mod mod, Object instance, File source) {
        ModMetadata meta = new ModMetadata();
        meta.modId = mod.modid();
        meta.name = mod.name().isEmpty() ? mod.modid() : mod.name();
        meta.version = mod.version();
        return Proxy.newProxyInstance(ModLoader.class.getClassLoader(), new Class<?>[] { ModContainer.class }, (proxy, method, args) -> {
            switch (method.getName()) {
                case "getModId": return meta.modId;
                case "getName": return meta.name;
                case "getVersion": case "getDisplayVersion": return meta.version;
                case "getMetadata": return meta;
                case "getSource": return source;
                case "getMod": return instance;
                case "toString": return "ModContainer[" + meta.modId + "]";
                case "hashCode": return System.identityHashCode(proxy);
                case "equals": return proxy == args[0];
            }
            Class<?> type = method.getReturnType();
            if (type == boolean.class) return Boolean.FALSE;
            if (type == int.class) return 0;
            if (type == long.class) return 0L;
            if (type == float.class) return 0f;
            if (type == double.class) return 0d;
            if (type == java.util.List.class) return new ArrayList<Object>();
            if (type == java.util.Set.class) return new java.util.HashSet<Object>();
            if (type == java.util.Map.class) return new java.util.HashMap<Object, Object>();
            return null;
        });
    }
}
