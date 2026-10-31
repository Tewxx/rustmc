package rustmc.forge;

import java.io.File;
import java.lang.reflect.Proxy;
import java.util.ArrayList;
import java.util.Enumeration;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;
import net.minecraftforge.fml.common.Mod;
import net.minecraftforge.fml.common.ModContainer;
import net.minecraftforge.fml.common.ModMetadata;
import org.apache.logging.log4j.Level;
import org.apache.logging.log4j.LogManager;

/**
 * FML state Rust cannot hold: the mods on the class path (what `Loader` answers) and FMLLog's log4j sink.
 * Discovery repeats {@link ModLoader}'s rules without constructing anything, so it is safe at any time.
 */
public final class Fml {
    private static List<Object> containers;
    private static Map<String, Object> byId;

    /** Every top-level class of a mod jar that loads and carries @Mod, in jar order (see ModLoader.load). */
    private static synchronized void ensure() {
        if (containers != null) return;
        List<Object> mods = new ArrayList<>();
        Map<String, Object> ids = new HashMap<>();
        for (String jar : System.getProperty("rustmc.modJars", "").split(File.pathSeparator)) {
            if (jar.isEmpty()) continue;
            File source = new File(jar);
            try (ZipFile zip = new ZipFile(source)) {
                for (Enumeration<? extends ZipEntry> e = zip.entries(); e.hasMoreElements();) {
                    String n = e.nextElement().getName();
                    if (!n.endsWith(".class") || n.contains("$")) continue;
                    try {
                        Class<?> c = Class.forName(n.substring(0, n.length() - 6).replace('/', '.'), false, Fml.class.getClassLoader());
                        Mod mod = c.getAnnotation(Mod.class);
                        if (mod == null) continue;
                        Object container = container(mod, source);
                        mods.add(container);
                        ids.put(mod.modid(), container);
                    } catch (Throwable t) {
                        // a class ModLoader could not even load (mixin, missing dependency) is not a mod
                    }
                }
            } catch (Throwable t) {
                System.err.println("[forge] cannot read mod jar " + jar);
            }
        }
        byId = ids;
        containers = mods;
    }

    /** Loader.isModLoaded: our mods, plus the ids FML always injects as mod containers. */
    public static boolean isLoaded(String id) {
        ensure();
        return byId.containsKey(id) || id.equals("mcp") || id.equals("FML") || id.equals("Forge");
    }

    /** Loader.getActiveModList. */
    public static List<Object> mods() {
        if (!ModLoader.LOADED.isEmpty()) return ModLoader.LOADED;
        ensure();
        return containers;
    }

    /** FMLCommonHandler.getBrandings: the client brand when asked for, then one entry per loaded mod. */
    public static List<String> brandings(boolean includeMc) {
        ensure();
        List<String> out = new ArrayList<>(containers.size() + 1);
        if (includeMc) out.add("mcp");
        for (Object c : containers) out.add(((ModContainer) c).getModId());
        return out;
    }

    /** FMLLog's backend: log4j2 under the "fml" logger, String.format placeholders like vanilla. */
    public static void log(Level level, Throwable t, String format, Object[] data) {
        LogManager.getLogger("fml").log(level, data == null || data.length == 0 ? format : String.format(format, data), t);
    }

    public static void fine(String format, Object[] data) {
        log(Level.DEBUG, null, format, data);
    }

    /** A ModContainer known only by its @Mod values (ModLoader.container, without the mod instance). */
    private static Object container(Mod mod, File source) {
        ModMetadata meta = new ModMetadata();
        meta.modId = mod.modid();
        meta.name = mod.name().isEmpty() ? mod.modid() : mod.name();
        meta.version = mod.version();
        return Proxy.newProxyInstance(Fml.class.getClassLoader(), new Class<?>[] { ModContainer.class }, (proxy, method, args) -> {
            switch (method.getName()) {
                case "getModId": return meta.modId;
                case "getName": return meta.name;
                case "getVersion": case "getDisplayVersion": return meta.version;
                case "getMetadata": return meta;
                case "getSource": return source;
                case "toString": return meta.modId;
                case "hashCode": return meta.modId.hashCode();
                case "equals": return proxy == args[0];
                default:
                    Class<?> r = method.getReturnType();
                    if (r == boolean.class) return false;
                    if (r == long.class) return 0L;
                    if (r == float.class) return 0F;
                    if (r == double.class) return 0D;
                    if (r.isPrimitive()) return 0;
                    return null;
            }
        });
    }
}
