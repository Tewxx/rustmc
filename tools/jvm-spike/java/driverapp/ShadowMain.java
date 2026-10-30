package driverapp;
import java.util.List;
import net.minecraft.launchwrapper.IClassTransformer;
import net.minecraft.launchwrapper.Launch;
import bridgeprobe.ShadowRun;
/** Verifies transformer order and captures final bytes; no game entrypoint is called. */
public final class ShadowMain {
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
        ShadowRun.target = Class.forName("net.minecraft.util.MathHelper", true, Launch.classLoader);
        ShadowRun.bridge = Class.forName("ft.shadow.NativeBridge", true, Launch.classLoader);
        System.out.println("selected shadow target with loader " + ShadowRun.target.getClassLoader());
    }
}
