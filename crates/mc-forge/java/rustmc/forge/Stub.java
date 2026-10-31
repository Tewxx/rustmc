package rustmc.forge;

/** Target of every generated stub body (tools/shadow/StubJar.java). Rust implements or logs the call. */
public final class Stub {
    /** Set by Rust once {@link #dispatch} is registered; until then stubs return null (zero). */
    private static volatile boolean bound;

    public static void bind() { bound = true; }

    /** key is "net/minecraft/client/Minecraft.func_71410_x()Lnet/minecraft/client/Minecraft;"; self is null for statics. */
    public static Object call(String key, Object self, Object[] args) {
        return bound ? dispatch(key, self, args) : null;
    }

    private static native Object dispatch(String key, Object self, Object[] args);

    public static boolean asBool(Object o) { return o != null && (Boolean) o; }
    public static int asInt(Object o) { return o == null ? 0 : o instanceof Character ? (Character) o : o instanceof Boolean ? ((Boolean) o ? 1 : 0) : ((Number) o).intValue(); }
    public static long asLong(Object o) { return o == null ? 0 : ((Number) o).longValue(); }
    public static float asFloat(Object o) { return o == null ? 0 : ((Number) o).floatValue(); }
    public static double asDouble(Object o) { return o == null ? 0 : ((Number) o).doubleValue(); }
}
