package rustmc.extract;

import com.google.gson.JsonArray;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import net.minecraft.client.renderer.RenderGlobal;
import net.minecraft.client.renderer.WorldRenderer;
import net.minecraft.client.renderer.vertex.VertexFormat;

/** Captures RenderGlobal's generated star vertices without starting a renderer. */
public final class DumpStars {
    private static class Capture extends WorldRenderer {
        final JsonArray vertices = new JsonArray();
        Capture() { super(16); }
        @Override public void begin(int mode, VertexFormat format) { }
        @Override public WorldRenderer pos(double x, double y, double z) {
            JsonArray vertex = new JsonArray();
            vertex.add(new com.google.gson.JsonPrimitive(Float.floatToIntBits((float)x))); vertex.add(new com.google.gson.JsonPrimitive(Float.floatToIntBits((float)y))); vertex.add(new com.google.gson.JsonPrimitive(Float.floatToIntBits((float)z)));
            vertices.add(vertex); return this;
        }
        @Override public void endVertex() { }
    }
    public static JsonArray dump() throws Exception {
        Field f = sun.misc.Unsafe.class.getDeclaredField("theUnsafe"); f.setAccessible(true);
        sun.misc.Unsafe unsafe = (sun.misc.Unsafe) f.get(null);
        RenderGlobal renderer = (RenderGlobal) unsafe.allocateInstance(RenderGlobal.class);
        Method render = RenderGlobal.class.getDeclaredMethod("renderStars", WorldRenderer.class); render.setAccessible(true);
        Capture capture = new Capture(); render.invoke(renderer, capture); return capture.vertices;
    }
}
