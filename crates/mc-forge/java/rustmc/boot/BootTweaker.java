package rustmc.boot;

import java.io.File;
import java.util.ArrayList;
import java.util.List;
import java.util.jar.JarFile;
import net.minecraft.launchwrapper.ITweaker;
import net.minecraft.launchwrapper.Launch;
import net.minecraft.launchwrapper.LaunchClassLoader;

/** Queues the tweakers named by mod jars (FML's CoreModManager does this on a real install), then ends the launch. */
public final class BootTweaker implements ITweaker {
    @SuppressWarnings("unchecked")
    public void acceptOptions(List<String> args, File gameDir, File assetsDir, String profile) {
        Launcher.onLoader();
        List<String> queue = (List<String>) Launch.blackboard.get("TweakClasses");
        for (String jar : System.getProperty("rustmc.modJars", "").split(File.pathSeparator)) {
            if (jar.isEmpty()) continue;
            try (JarFile f = new JarFile(jar)) {
                String tweak = f.getManifest() == null ? null : f.getManifest().getMainAttributes().getValue("TweakClass");
                if (tweak != null && !queue.contains(tweak)) queue.add(tweak);
            } catch (java.io.IOException e) {
                System.err.println("[forge] cannot read " + jar + ": " + e);
            }
        }
    }

    public void injectIntoClassLoader(LaunchClassLoader loader) {
        loader.registerTransformer("rustmc.boot.AwtTransformer");
    }

    public String getLaunchTarget() { return "rustmc.boot.Boot"; }

    public String[] getLaunchArguments() { return new String[0]; }
}
