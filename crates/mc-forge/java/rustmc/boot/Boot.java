package rustmc.boot;

/** LaunchWrapper's launch target: runs after every tweaker is injected, before any game class loads. */
@SuppressWarnings({"unchecked", "rawtypes"})
public final class Boot {
    public static void main(String[] args) {
        // Without FML, Mixin cannot tell it is on the client and would skip every "client" mixin list.
        try {
            Class<?> env = Class.forName("org.spongepowered.asm.mixin.MixinEnvironment");
            Class<?> side = Class.forName("org.spongepowered.asm.mixin.MixinEnvironment$Side");
            env.getMethod("setSide", side).invoke(env.getMethod("getDefaultEnvironment").invoke(null), Enum.valueOf((Class) side, "CLIENT"));
        } catch (ClassNotFoundException e) {
            // no Mixin on the class path
        } catch (java.lang.reflect.InvocationTargetException e) {
            // "didn't call MixinBootstrap.init()": no mod queued MixinTweaker, so Mixin is not in use.
            String why = String.valueOf(e.getCause().getMessage());
            if (!why.contains("MixinBootstrap.init")) System.err.println("[forge] cannot select Mixin client side: " + e.getCause());
        } catch (Exception e) {
            System.err.println("[forge] cannot select Mixin client side: " + e);
        }
    }
}
