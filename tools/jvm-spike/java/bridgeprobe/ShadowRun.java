package bridgeprobe;
/** Holds classes returned by the child LaunchClassLoader for the Rust JNI driver. */
public final class ShadowRun {
    public static Class<?> target;
    public static Class<?> bridge;
    public static Class<?> eventBridge;
    public static Class<?> tickBridge;
    public static int value() {
        System.setProperty("rustmc.headlessProbe", "true");
        System.setProperty("fml.coreMods.load", "ft.shadow.Plugin");
        System.setProperty("mixin.debug.export", "true");
        net.minecraft.launchwrapper.Launch.main(new String[] {"--tweakClass", "bridgeprobe.HeadlessTweaker", "--gameDir", System.getProperty("user.dir"), "--version", "1.8.9"});
        if (target == null || bridge == null || eventBridge == null || tickBridge == null) throw new IllegalStateException("headless Forge bootstrap did not finish");
        return 1;
    }
}
