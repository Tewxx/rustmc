package rustmc.boot;

import java.io.File;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.jar.JarFile;
import net.minecraft.launchwrapper.ITweaker;
import net.minecraft.launchwrapper.Launch;
import net.minecraft.launchwrapper.LaunchClassLoader;

/** Queues the tweakers named by mod jars and loads their FML coremods (FML's CoreModManager does both on a real
 *  install), then ends the launch. */
public final class BootTweaker implements ITweaker {
    /** (jar, FMLCorePlugin class) in manifest order. */
    private final List<String[]> corePlugins = new ArrayList<>();

    @SuppressWarnings("unchecked")
    public void acceptOptions(List<String> args, File gameDir, File assetsDir, String profile) {
        Launcher.onLoader();
        List<String> queue = (List<String>) Launch.blackboard.get("TweakClasses");
        for (String jar : System.getProperty("rustmc.modJars", "").split(File.pathSeparator)) {
            if (jar.isEmpty()) continue;
            try (JarFile f = new JarFile(jar)) {
                String tweak = f.getManifest() == null ? null : f.getManifest().getMainAttributes().getValue("TweakClass");
                if (tweak != null && !queue.contains(tweak)) queue.add(tweak);
                String core = f.getManifest() == null ? null : f.getManifest().getMainAttributes().getValue("FMLCorePlugin");
                if (core != null) corePlugins.add(new String[] {jar, core.trim()});
            } catch (java.io.IOException e) {
                System.err.println("[forge] cannot read " + jar + ": " + e);
            }
        }
    }

    public void injectIntoClassLoader(LaunchClassLoader loader) {
        loader.registerTransformer("rustmc.boot.AwtTransformer");
        injectionData(loader);
        if (!corePlugins.isEmpty()) {
            // FML's own exclusions: a coremod transformer that calls FMLLaunchHandler.side() must not transform the
            // class it is loading (ClassCircularityError). Our glue is never a transform target either.
            for (String p : new String[] {"net.minecraftforge.fml.relauncher.", "net.minecraftforge.fml.repackage.",
                    "net.minecraftforge.fml.common.asm.transformers.", "net.minecraftforge.classloading.", "rustmc."})
                loader.addTransformerExclusion(p);
        }
        for (String[] plugin : corePlugins) loadCorePlugin(loader, new File(plugin[0]), plugin[1]);
    }

    /** CoreModManager.loadCoreMod + the plugin's setup: exclusions, transformers, call hook, injectData. Names are SRG
     *  at runtime, so runtimeDeobfuscationEnabled is true. A broken coremod is reported and skipped. */
    private static void loadCorePlugin(LaunchClassLoader loader, File jar, String name) {
        try {
            String pkg = name.lastIndexOf('.') < 0 ? "" : name.substring(0, name.lastIndexOf('.'));
            loader.addTransformerExclusion(name);
            Class<?> type = Class.forName(name, true, loader);
            // The plugin's interfaces come from the LaunchClassLoader, not ours, so everything goes through reflection.
            for (java.lang.annotation.Annotation a : type.getAnnotations())
                if (a.annotationType().getName().endsWith("IFMLLoadingPlugin$TransformerExclusions"))
                    for (String e : (String[]) a.annotationType().getMethod("value").invoke(a)) loader.addTransformerExclusion(e);
            Object plugin = type.newInstance();
            Map<String, Object> data = new HashMap<>();
            data.put("mcLocation", Launch.minecraftHome);
            data.put("coremodList", new ArrayList<Object>());
            data.put("runtimeDeobfuscationEnabled", Boolean.TRUE);
            data.put("coremodLocation", jar);
            String[] transformers = (String[]) call(plugin, "getASMTransformerClass");
            if (transformers != null) for (String t : transformers) loader.registerTransformer(t);
            String access = (String) call(plugin, "getAccessTransformerClass");
            if (access != null) loader.registerTransformer(access);
            String setup = (String) call(plugin, "getSetupClass");
            if (setup != null) {
                Object hook = Class.forName(setup, true, loader).newInstance();
                Map<String, Object> hookData = new HashMap<>(data);
                hookData.put("classLoader", loader);
                hook.getClass().getMethod("injectData", Map.class).invoke(hook, hookData);
                hook.getClass().getMethod("call").invoke(hook);
            }
            plugin.getClass().getMethod("injectData", Map.class).invoke(plugin, data);
            System.out.println("[forge] coremod " + name + " (" + pkg + ") from " + jar.getName());
        } catch (Throwable t) {
            System.err.println("[forge] coremod " + name + " failed: " + t);
            t.printStackTrace();
        }
    }

    /** FMLLaunchHandler.setup's FMLInjectionData.build: minecraftHome and the versions Configuration and mods read
     *  through FMLInjectionData.data(). The jar carries no fmlversion.properties, so the MC/MCP versions are set here. */
    private static void injectionData(LaunchClassLoader loader) {
        try {
            Class<?> data = Class.forName("net.minecraftforge.fml.relauncher.FMLInjectionData", true, loader);
            java.lang.reflect.Method build = data.getDeclaredMethod("build", File.class, LaunchClassLoader.class);
            build.setAccessible(true);
            build.invoke(null, Launch.minecraftHome == null ? new File(".") : Launch.minecraftHome, loader);
            for (String[] f : new String[][] {{"mccversion", "1.8.9"}, {"mcpversion", "9.19"}}) {
                java.lang.reflect.Field field = data.getDeclaredField(f[0]);
                field.setAccessible(true);
                if (field.get(null) == null || "missing".equals(field.get(null))) field.set(null, f[1]);
            }
        } catch (Throwable t) {
            System.err.println("[forge] FMLInjectionData setup failed: " + t);
        }
    }

    private static Object call(Object target, String method) throws Exception { return target.getClass().getMethod(method).invoke(target); }

    public String getLaunchTarget() { return "rustmc.boot.Boot"; }

    public String[] getLaunchArguments() { return new String[0]; }
}
