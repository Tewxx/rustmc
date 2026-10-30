package spikecapture;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import net.minecraft.launchwrapper.IClassTransformer;
/** Last in the chain for this probe: save exactly the bytes passed to class definition. */
public final class CaptureTransformer implements IClassTransformer {
    public byte[] transform(String name, String transformedName, byte[] bytes) {
        if ("net.minecraft.util.MathHelper".equals(transformedName) && bytes != null) {
            try {
                Path out = Paths.get(System.getProperty("user.dir"), "final-MathHelper.class");
                Files.write(out, bytes);
            } catch (Exception e) { throw new IllegalStateException("cannot capture transformed class", e); }
        }
        return bytes;
    }
}
