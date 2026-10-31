package spikecapture;
import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.jar.JarFile;
import net.minecraft.launchwrapper.IClassTransformer;
/** Probe-only selector: replace selected targets with generated shadow bytes before Mixin. */
public final class ShadowSelectTransformer implements IClassTransformer {
    public byte[] transform(String name, String transformedName, byte[] bytes) {
        boolean graph = "1".equals(System.getenv("RUSTMC_SHADOW_GRAPH"));
        if (graph ? !transformedName.startsWith("net.minecraft.") : !"net.minecraft.util.MathHelper".equals(transformedName)) return bytes;
        try (JarFile jar = new JarFile(System.getenv("RUSTMC_SHADOW_JAR"))) {
            java.util.jar.JarEntry entry = jar.getJarEntry(transformedName.replace('.', '/') + ".class");
            if (entry == null) {
                if (graph && bytes != null) throw new IllegalStateException("shadow missing available class " + transformedName);
                if (graph) Files.write(Paths.get(System.getProperty("user.dir"), "shadow-missing.txt"),
                    (transformedName + "\n").getBytes(java.nio.charset.StandardCharsets.UTF_8),
                    java.nio.file.StandardOpenOption.CREATE, java.nio.file.StandardOpenOption.APPEND);
                return bytes;
            }
            ByteArrayOutputStream out = new ByteArrayOutputStream();
            try (InputStream in = jar.getInputStream(entry)) {
                byte[] buffer = new byte[8192]; int n;
                while ((n = in.read(buffer)) != -1) out.write(buffer, 0, n);
            }
            byte[] selected = out.toByteArray();
            if ("net.minecraft.util.MathHelper".equals(transformedName)) Files.write(Paths.get(System.getProperty("user.dir"), "selected-MathHelper.class"), selected);
            if (graph) Files.write(Paths.get(System.getProperty("user.dir"), "shadow-selected.txt"),
                (transformedName + "\n").getBytes(java.nio.charset.StandardCharsets.UTF_8),
                java.nio.file.StandardOpenOption.CREATE, java.nio.file.StandardOpenOption.APPEND);
            return selected;
        } catch (Exception e) { throw new IllegalStateException("cannot select shadow target", e); }
    }
}
