package spikecapture;
import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.jar.JarFile;
import net.minecraft.launchwrapper.IClassTransformer;
/** Probe-only selector: replace one target with the generated shadow before Mixin. */
public final class ShadowSelectTransformer implements IClassTransformer {
    public byte[] transform(String name, String transformedName, byte[] bytes) {
        if (!"net.minecraft.util.MathHelper".equals(transformedName)) return bytes;
        try (JarFile jar = new JarFile(System.getenv("RUSTMC_SHADOW_JAR"))) {
            java.util.jar.JarEntry entry = jar.getJarEntry("net/minecraft/util/MathHelper.class");
            if (entry == null) throw new IllegalStateException("shadow target missing");
            ByteArrayOutputStream out = new ByteArrayOutputStream();
            try (InputStream in = jar.getInputStream(entry)) {
                byte[] buffer = new byte[8192]; int n;
                while ((n = in.read(buffer)) != -1) out.write(buffer, 0, n);
            }
            byte[] selected = out.toByteArray();
            Files.write(Paths.get(System.getProperty("user.dir"), "selected-MathHelper.class"), selected);
            return selected;
        } catch (Exception e) { throw new IllegalStateException("cannot select shadow target", e); }
    }
}
