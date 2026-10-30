package rustmc.forge;

import java.io.File;
import java.lang.reflect.Constructor;
import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.Enumeration;
import java.util.List;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;
import net.minecraftforge.fml.common.Mod;
import net.minecraftforge.fml.common.event.FMLInitializationEvent;
import net.minecraftforge.fml.common.event.FMLPostInitializationEvent;
import net.minecraftforge.fml.common.event.FMLPreInitializationEvent;

/** Finds @Mod classes in jars already on the class path and runs their pre-init, init and post-init handlers. */
public final class ModLoader {
    private static final File CONFIG = new File("config");

    /** Returns the ids of the mods that ran. A mod that throws is reported and skipped; the others still load. */
    public static String[] load(String[] jars) throws Exception {
        List<Object> mods = new ArrayList<>();
        List<String> ids = new ArrayList<>();
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
                    try { mods.add(c.newInstance()); ids.add(mod.modid()); }
                    catch (Throwable t) { System.err.println("[forge] cannot construct mod " + mod.modid()); t.printStackTrace(); }
                }
            }
        }
        // Events share one shape: a varargs Object[] constructor fed (asmData, configDir, configFile / unused).
        Object[] data = { null, CONFIG, new File(CONFIG, "mod.cfg") };
        for (Class<?> phase : new Class<?>[] { FMLPreInitializationEvent.class, FMLInitializationEvent.class, FMLPostInitializationEvent.class }) {
            Constructor<?> k = phase.getConstructor(Object[].class);
            for (Object mod : mods) for (Method m : mod.getClass().getMethods()) {
                if (!m.isAnnotationPresent(Mod.EventHandler.class) || m.getParameterTypes().length != 1 || m.getParameterTypes()[0] != phase) continue;
                try { m.invoke(mod, k.newInstance((Object) data)); }
                catch (Throwable t) { System.err.println("[forge] " + m + " failed"); t.printStackTrace(); }
            }
        }
        return ids.toArray(new String[0]);
    }
}
