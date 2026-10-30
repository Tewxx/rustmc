package spike;
/** Crosses back to the system class loader via a property after LaunchWrapper returns. */
public class Run {
    public static Object target;
    public static void install(Object value) { target = value; }
    public static int value() {
        net.minecraft.launchwrapper.Launch.main(new String[] {"--tweakClass", "spike.Tweaker"});
        int result = Integer.parseInt(System.getProperty("rustmc.spike.value", "-1"));
        if (result != 2) throw new IllegalStateException("Mixin did not transform target: " + result);
        return result;
    }
}
