package rustmc.boot;

import net.minecraft.launchwrapper.Launch;

/**
 * Boots LaunchWrapper once (system class loader). Everything else lives in Launch.classLoader.
 * LaunchWrapper excludes the tweaker's package from that loader, so rustmc.boot stays separate from rustmc.forge.
 */
public final class Launcher {
    /** Implemented by Rust: LaunchClassLoader exists now, so bind the stubs before any tweaker (Mixin) runs. */
    static native void onLoader();

    /** @param modJars class-path jars that are mods; their manifest TweakClass entries (Mixin...) are launched. */
    public static void start(String[] modJars) {
        System.setProperty("rustmc.modJars", String.join(java.io.File.pathSeparator, modJars));
        Launch.main(new String[] { "--tweakClass", "rustmc.boot.BootTweaker", "--gameDir", System.getProperty("user.dir"), "--version", "1.8.9" });
    }
}
