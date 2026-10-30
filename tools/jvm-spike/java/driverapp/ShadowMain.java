package driverapp;
import java.lang.reflect.Field;
import java.util.List;
import net.minecraft.launchwrapper.IClassTransformer;
import net.minecraft.launchwrapper.Launch;
import bridgeprobe.ShadowRun;
/** Installs a one-class shadow selector and a final-byte capture; no game entrypoint is called. */
public final class ShadowMain {
    @SuppressWarnings("unchecked")
    public static void main(String[] args) throws Exception {
        // In this probe only, inject the selector after Forge's patcher but before Mixin.
        // Production needs a supported, general loader integration rather than reflective surgery.
        Launch.classLoader.registerTransformer("spikecapture.ShadowSelectTransformer");
        Field f = Launch.classLoader.getClass().getDeclaredField("transformers");
        f.setAccessible(true);
        List<IClassTransformer> transformers = (List<IClassTransformer>) f.get(Launch.classLoader);
        IClassTransformer selector = transformers.remove(transformers.size() - 1);
        int index = 0;
        while (index < transformers.size() && !transformers.get(index).getClass().getName().equals("org.spongepowered.asm.mixin.transformer.Proxy")) index++;
        if (index == 0 || index == transformers.size() || !transformers.get(0).getClass().getName().endsWith("PatchingTransformer"))
            throw new IllegalStateException("unexpected Forge/Mixin transformer order");
        transformers.add(index, selector);
        Launch.classLoader.registerTransformer("spikecapture.CaptureTransformer");
        ShadowRun.target = Class.forName("net.minecraft.util.MathHelper", true, Launch.classLoader);
        ShadowRun.bridge = Class.forName("ft.shadow.NativeBridge", true, Launch.classLoader);
        System.out.println("selected shadow target with loader " + ShadowRun.target.getClassLoader());
    }
}
