package driverapp;
import java.util.List;
import net.minecraft.launchwrapper.IClassTransformer;
import net.minecraft.launchwrapper.Launch;
import bridgeprobe.ShadowRun;
/** Verifies transformer order and captures final bytes; no game entrypoint is called. */
public final class ShadowMain {
    @SuppressWarnings({"unchecked", "rawtypes"})
    public static void main(String[] args) throws Exception {
        List<IClassTransformer> transformers = Launch.classLoader.getTransformers();
        int patch = -1, selector = -1, mixin = -1;
        for (int i = 0; i < transformers.size(); i++) {
            String name = transformers.get(i).getClass().getName();
            if (name.endsWith("PatchingTransformer")) patch = i;
            if (name.equals("spikecapture.ShadowSelectTransformer")) selector = i;
            if (name.equals("org.spongepowered.asm.mixin.transformer.Proxy") && mixin == -1) mixin = i;
        }
        if (!(patch >= 0 && patch < selector && selector < mixin)) throw new IllegalStateException("unexpected transformer order: " + transformers);
        Launch.classLoader.registerTransformer("spikecapture.CaptureTransformer");
        Class.forName("net.minecraft.init.Bootstrap", true, Launch.classLoader).getMethod("func_151354_b").invoke(null);
        Class<?> handler = Class.forName("net.minecraftforge.fml.common.FMLCommonHandler", true, Launch.classLoader);
        Object common = handler.getMethod("instance").invoke(null);
        Class<?> sided = Class.forName("net.minecraftforge.fml.common.IFMLSidedHandler", true, Launch.classLoader);
        Object client = java.lang.reflect.Proxy.newProxyInstance(Launch.classLoader, new Class<?>[]{sided}, (proxy, method, argv) -> {
            switch (method.getName()) {
                case "getSide": return Enum.valueOf((Class) Class.forName("net.minecraftforge.fml.relauncher.Side", true, Launch.classLoader), "CLIENT");
                case "addModAsResource": case "processWindowMessages": return null;
                case "stripSpecialChars": return ((String) argv[0]).replaceAll("(?i)\u00a7[0-9A-FK-OR]", "");
                default: throw new UnsupportedOperationException("unsupported headless client call: " + method.getName());
            }
        });
        // Mixin 0.8.7 contains newer class files that Forge's ASM 5 discovery cannot parse.
        // It is a runtime library, not a mod candidate.
        ((List<String>) Class.forName("net.minecraftforge.fml.relauncher.CoreModManager", true, Launch.classLoader)
            .getMethod("getIgnoredMods").invoke(null)).add("mixin-0.8.7.jar");
        handler.getMethod("beginLoading", sided).invoke(common, client);
        Object loader = Class.forName("net.minecraftforge.fml.common.Loader", true, Launch.classLoader).getMethod("instance").invoke(null);
        loader.getClass().getMethod("loadMods").invoke(loader);
        loader.getClass().getMethod("preinitializeMods").invoke(loader);
        loader.getClass().getMethod("initializeMods").invoke(loader);
        loader.getClass().getMethod("loadingComplete").invoke(loader);
        if (!"AVAILABLE".equals(loader.getClass().getMethod("getLoaderState").invoke(loader).toString()))
            throw new IllegalStateException("FML lifecycle did not reach AVAILABLE");
        ShadowRun.target = Class.forName("net.minecraft.util.MathHelper", true, Launch.classLoader);
        ShadowRun.bridge = Class.forName("ft.shadow.NativeBridge", true, Launch.classLoader);
        ShadowRun.eventBridge = Class.forName("ft.bus.FTBus", true, Launch.classLoader);
        System.out.println("selected shadow target with loader " + ShadowRun.target.getClassLoader());
    }
}
