package rustmc.forge;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.lang.reflect.Field;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.Map;

/**
 * The post-processing objects EntityRenderer hands out: a real {@code ShaderGroup} holding a real
 * {@code Shader} per JSON pass, each with a {@code ShaderManager} of {@code ShaderUniform}s, so mods
 * can walk {@code field_148031_d} and call {@code getShaderManager().getShaderUniform("Radius").set(f)}.
 * The numbers they set are read back by Rust each frame ({@link #snapshot}) and uploaded by
 * {@code mc_render::post}; no GL is involved.
 */
public final class ShaderGlue {
    /** ShaderManager -> (uniform name -> ShaderUniform). */
    private static final Map<Object, Map<String, Object>> UNIFORMS = new IdentityHashMap<Object, Map<String, Object>>();
    /** Shader -> ShaderManager (Shader.func_148043_c). */
    private static final Map<Object, Object> MANAGERS = new IdentityHashMap<Object, Object>();
    /** ShaderUniform -> its four floats (vanilla keeps a FloatBuffer of the declared count). */
    private static final Map<Object, float[]> VALUES = new IdentityHashMap<Object, float[]>();
    /** ShaderUniform -> declared name (ShaderUniform.func_148086_a). */
    private static final Map<Object, String> NAMES = new IdentityHashMap<Object, String>();

    /** Vanilla uniform names per program, for when the program JSON cannot be read from a resource pack. */
    private static final Map<String, String[]> KNOWN = new HashMap<String, String[]>();
    static {
        String[] common = { "ProjMat", "InSize", "OutSize", "ScreenSize", "Time" };
        KNOWN.put("blur", new String[] { "ProjMat", "InSize", "OutSize", "ScreenSize", "Time", "BlurDir", "Radius" });
        KNOWN.put("blit", new String[] { "ProjMat", "InSize", "OutSize", "ScreenSize", "Time", "ColorModulate" });
        KNOWN.put("phosphor", new String[] { "ProjMat", "InSize", "OutSize", "ScreenSize", "Time", "Phosphor" });
        KNOWN.put("notch", new String[] { "ProjMat", "InSize", "OutSize", "ScreenSize", "Time", "Resolution" });
        KNOWN.put("", common);
    }

    private static Object group;
    private static Object renderer;
    private static String location;
    private static String json;
    private static int width;
    private static int height;
    /** "name\u0001json" per pass program we managed to read. */
    private static final List<String[]> PROGRAMS = new ArrayList<String[]>();

    // ---- called from impls/shader.rs ------------------------------------------------------

    /** EntityRenderer.loadShader: build the group for `path` ("minecraft:shaders/post/blur.json"). */
    public static Object load(Object rendererIn, String path) throws Exception {
        stop();
        String post = read(path);
        renderer = rendererIn;
        location = path;
        json = post == null ? "" : post;
        Object created = allocate("net.minecraft.client.shader.ShaderGroup");
        List<Object> shaders = new ArrayList<Object>();
        if (post != null) {
            JsonObject root = object(new JsonParser().parse(post));
            for (JsonElement e : array(root, "passes")) {
                JsonObject pass = object(e);
                String name = pass.has("name") ? pass.get("name").getAsString() : "";
                String program = read("minecraft:shaders/program/" + name + ".json");
                if (program != null) PROGRAMS.add(new String[] { name, program });
                shaders.add(shader(name, program, pass));
            }
        }
        set(created, "field_148031_d", shaders);
        width = display("field_71443_c");
        height = display("field_71440_d");
        group = created;
        // Keep the vanilla fields in sync for mods that read them directly.
        try { set(renderer, "field_147707_d", created); } catch (Exception noField) { }
        try { set(renderer, "field_175083_a", Boolean.TRUE); } catch (Exception noField) { }
        return created;
    }

    /** EntityRenderer.stopUseShader / ShaderGroup.func_148021_a. */
    public static void stop() {
        try { if (renderer != null) { set(renderer, "field_147707_d", null); set(renderer, "field_175083_a", Boolean.FALSE); } }
        catch (Exception noField) { }
        group = null;
        location = null;
        json = null;
        PROGRAMS.clear();
        UNIFORMS.clear();
        MANAGERS.clear();
        VALUES.clear();
        NAMES.clear();
    }

    public static Object group() { return group; }
    public static String location() { return location; }
    public static String json() { return json; }

    /** "name\u0001json\u0002" for every pass program read from a resource pack. */
    public static String programs() {
        StringBuilder sb = new StringBuilder();
        for (String[] entry : PROGRAMS) sb.append(entry[0]).append('\u0001').append(entry[1]).append('\u0002');
        return sb.toString();
    }

    /** "width\theight\n" then "name\tv0,v1\n" for every uniform, i.e. what the mod set this frame. */
    public static String snapshot() {
        StringBuilder sb = new StringBuilder();
        sb.append(width).append('\t').append(height).append('\n');
        for (Map<String, Object> map : UNIFORMS.values()) {
            for (Map.Entry<String, Object> e : map.entrySet()) {
                float[] v = VALUES.get(e.getValue());
                if (v == null) continue;
                sb.append(e.getKey()).append('\t');
                for (int i = 0; i < v.length; i++) {
                    if (i > 0) sb.append(',');
                    sb.append(v[i]);
                }
                sb.append('\n');
            }
        }
        return sb.toString();
    }

    // ---- dispatch of the shader classes --------------------------------------------------

    public static Object invoke(String key, Object self, Object[] args) throws Exception {
        String method = key.substring(key.lastIndexOf('.') + 1);
        if (SET.contains(method)) return setUniform(self, args);          // ShaderUniform.set(...)
        if (method.equals("func_148086_a")) return NAMES.get(self);       // ShaderUniform.getShaderName
        if (method.equals("func_148043_c")) return MANAGERS.get(self);    // Shader.getShaderManager
        if (method.equals("func_147991_a")) return uniforms(self).get((String) args[0]); // ShaderManager.getShaderUniform
        if (method.equals("func_147984_b")) return orDefault(self, (String) args[0]);    // getShaderUniformOrDefault
        if (method.equals("func_148021_a")) return null;                  // ShaderGroup.deleteShaderGroup
        if (method.equals("func_148026_a")) {                             // ShaderGroup.createBindFramebuffers
            width = number(args[0]);
            height = number(args[1]);
            return null;
        }
        if (method.equals("getShaders")) return get(self, "field_148031_d");
        return null;
    }

    /** ShaderUniform's setters: float, vec2, vec3, vec4, mat4 (16 floats), ivec4 and float[]. */
    private static final java.util.Set<String> SET = new java.util.HashSet<String>(java.util.Arrays.asList(
        "func_148090_a", "func_148087_a", "func_148095_a", "func_148081_a", "func_148094_a", "func_148097_a",
        "func_148083_a", "func_148092_b", "func_148084_b"));

    private static Object shader(String program, String programJson, JsonObject pass) throws Exception {
        Object shader = allocate("net.minecraft.client.shader.Shader");
        Object manager = allocate("net.minecraft.client.shader.ShaderManager");
        Map<String, Object> uniforms = new HashMap<String, Object>();
        // ShaderManager.addUniform from the program JSON, then ShaderGroup.initUniform from the pass.
        if (programJson != null) {
            for (JsonElement e : array(object(new JsonParser().parse(programJson)), "uniforms")) add(uniforms, object(e));
        } else {
            String[] names = KNOWN.containsKey(program) ? KNOWN.get(program) : KNOWN.get("");
                for (String name : names) uniforms.put(name, uniform(name));
        }
        for (JsonElement e : array(pass, "uniforms")) add(uniforms, object(e));
        UNIFORMS.put(manager, uniforms);
        MANAGERS.put(shader, manager);
        return shader;
    }

    private static void add(Map<String, Object> uniforms, JsonObject uniform) throws Exception {
        if (!uniform.has("name")) return;
        String name = uniform.get("name").getAsString();
        Object object = uniforms.get(name);
        if (object == null) { object = uniform(name); uniforms.put(name, object); }
        float[] values = VALUES.get(object);
        JsonElement element = uniform.has("values") ? uniform.get("values") : uniform.get("value");
        if (element == null) return;
        if (element.isJsonArray()) {
            JsonArray array = element.getAsJsonArray();
            for (int i = 0; i < array.size() && i < values.length; i++) values[i] = array.get(i).getAsFloat();
        } else {
            values[0] = element.getAsFloat();
        }
    }

    private static Object uniform(String name) throws Exception {
        Object uniform = allocate("net.minecraft.client.shader.ShaderUniform");
        VALUES.put(uniform, new float[4]);
        NAMES.put(uniform, name);
        return uniform;
    }

    private static Map<String, Object> uniforms(Object manager) {
        Map<String, Object> map = UNIFORMS.get(manager);
        if (map == null) { map = new HashMap<String, Object>(); UNIFORMS.put(manager, map); }
        return map;
    }

    private static Object orDefault(Object manager, String name) throws Exception {
        Map<String, Object> map = uniforms(manager);
        Object uniform = map.get(name);
        if (uniform == null) { uniform = uniform(name); map.put(name, uniform); }
        return uniform;
    }

    /** ShaderUniform.set(...): floats, or one float array, into the value buffer. */
    private static Object setUniform(Object uniform, Object[] args) {
        float[] values = VALUES.get(uniform);
        if (values == null) { values = new float[4]; VALUES.put(uniform, values); }
        for (int i = 0; i < values.length; i++) values[i] = 0.0F;
        if (args.length == 1 && args[0] instanceof float[]) {
            float[] array = (float[]) args[0];
            for (int i = 0; i < array.length && i < values.length; i++) values[i] = array[i];
            return null;
        }
        for (int i = 0; i < args.length && i < values.length; i++) {
            if (args[i] instanceof Number) values[i] = ((Number) args[i]).floatValue();
        }
        return null;
    }

    // ---- resources -----------------------------------------------------------------------

    /** Reads "assets/&lt;domain&gt;/&lt;path&gt;" from the mod jars / classpath; null when absent. */
    public static String read(String path) {
        String file = path == null ? "" : path;
        String domain = "minecraft";
        int colon = file.indexOf(':');
        if (colon > 0) { domain = file.substring(0, colon); file = file.substring(colon + 1); }
        if (file.startsWith("/")) file = file.substring(1);
        String resource = "assets/" + domain + "/" + file;
        for (ClassLoader loader : new ClassLoader[] { ShaderGlue.class.getClassLoader(), Thread.currentThread().getContextClassLoader() }) {
            if (loader == null) continue;
            InputStream in = loader.getResourceAsStream(resource);
            if (in == null) continue;
            try {
                ByteArrayOutputStream out = new ByteArrayOutputStream();
                byte[] buffer = new byte[4096];
                for (int n; (n = in.read(buffer)) > 0; ) out.write(buffer, 0, n);
                return new String(out.toByteArray(), "UTF-8");
            } catch (Exception failed) {
                return null;
            } finally {
                try { in.close(); } catch (Exception ignored) { }
            }
        }
        return null;
    }

    // ---- reflection helpers (Tab's are private) ------------------------------------------

    private static Object allocate(String name) throws Exception {
        return Stub.call("rustmc.allocate", null, new Object[] { Class.forName(name, true, ShaderGlue.class.getClassLoader()) });
    }

    private static JsonObject object(JsonElement element) { return element.getAsJsonObject(); }

    private static JsonArray array(JsonObject object, String name) {
        JsonElement element = object == null ? null : object.get(name);
        return element != null && element.isJsonArray() ? element.getAsJsonArray() : new JsonArray();
    }

    private static int number(Object value) { return value instanceof Number ? ((Number) value).intValue() : 0; }

    private static int display(String field) {
        try {
            Object mc = Class.forName("net.minecraft.client.Minecraft", true, ShaderGlue.class.getClassLoader()).getMethod("func_71410_x").invoke(null);
            return mc == null ? 0 : ((Number) get(mc, field)).intValue();
        } catch (Exception noDisplay) {
            return 0;
        }
    }

    private static Field field(Class<?> type, String name) throws NoSuchFieldException {
        for (Class<?> c = type; c != null; c = c.getSuperclass()) {
            try { Field found = c.getDeclaredField(name); found.setAccessible(true); return found; }
            catch (NoSuchFieldException next) { }
        }
        throw new NoSuchFieldException(type.getName() + "." + name);
    }

    private static Object get(Object object, String name) throws Exception { return field(object.getClass(), name).get(object); }

    private static void set(Object object, String name, Object value) throws Exception { field(object.getClass(), name).set(object, value); }
}
