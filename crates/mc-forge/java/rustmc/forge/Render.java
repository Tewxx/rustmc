package rustmc.forge;

import com.mojang.authlib.GameProfile;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.UUID;
import javax.imageio.ImageIO;
import net.minecraft.client.renderer.entity.RenderManager;
import net.minecraft.client.renderer.entity.RenderPlayer;
import net.minecraft.util.ResourceLocation;

/** Model and entity renderer state for the stubbed classes. No box geometry is tessellated yet: the calls only
 *  keep the Java fields a mod reads (rotation points, texture size, children, box name) in vanilla shape. */
public final class Render {
    private static final Map<String, Field> FIELDS = new HashMap<String, Field>();

    public static Object invoke(String key, Object self, Object[] args) throws Exception {
        String cls = key.substring(0, key.lastIndexOf('.')).replace('/', '.');
        String method = key.substring(key.lastIndexOf('.') + 1);
        if (cls.equals("net.minecraft.client.model.ModelBase") && !method.equals("<init>")) {
            @SuppressWarnings("unchecked") Map<String, Object> offsets = (Map<String, Object>) get(self, "field_78094_a");
            if (offsets == null) { offsets = new HashMap<String, Object>(); set(self, "field_78094_a", offsets); }
            if (method.equals("func_78084_a")) return offsets.get(args[0]); // getTextureOffset
            Object offset = new net.minecraft.client.model.TextureOffset((Integer) args[1], (Integer) args[2]); // setTextureOffset
            set(offset, "field_78783_a", args[1]); set(offset, "field_78782_b", args[2]);
            offsets.put((String) args[0], offset);
            return null;
        }
        if (cls.equals("net.minecraft.client.renderer.entity.layers.LayerBipedArmor")) { // initArmor
            set(self, "field_177189_c", new net.minecraft.client.model.ModelBiped(0.5F));
            set(self, "field_177186_d", new net.minecraft.client.model.ModelBiped(1.0F));
            return null;
        }
        if (cls.equals("net.minecraft.client.model.ModelBase")) {
            set(self, "field_78090_t", 64); // textureWidth
            set(self, "field_78089_u", 32); // textureHeight
            list(self, "field_78092_r"); // boxList: ModelRenderer registers itself in it
            return null;
        }
        if (cls.equals("net.minecraft.client.model.ModelRenderer")) return renderer(self, method, args);
        if (cls.equals("net.minecraft.client.model.ModelPlayer") && !method.equals("<init>")) return null; // renderCape
        if (cls.equals("net.minecraft.client.renderer.ItemRenderer")) return null; // first-person hand
        if (cls.equals("net.minecraft.client.renderer.texture.TextureUtil")) {
            return args[0] == null ? null : ImageIO.read((java.io.InputStream) args[0]);
        }
        if (cls.equals("net.minecraft.client.resources.DefaultPlayerSkin")) return skinFor((UUID) args[0]);
        if (cls.equals("net.minecraft.client.resources.SkinManager")) {
            skinAvailable(args);
            return null;
        }
        if (method.equals("<init>")) return construct(cls, self, args);
        if (cls.equals("net.minecraft.client.renderer.entity.RendererLivingEntity")) { // addLayer / removeLayer
            @SuppressWarnings("unchecked") List<Object> layers = (List<Object>) list(self, "field_177097_h");
            return method.equals("func_177094_a") ? layers.add(args[0]) : layers.remove(args[0]);
        }
        if (cls.equals("net.minecraft.client.renderer.entity.RenderPlayer")) return get(self, "field_77045_g"); // mainModel
        if (cls.equals("net.minecraft.client.renderer.entity.RenderManager")) return manager(self, method, args);
        return null;
    }

    private static Object renderer(Object self, String method, Object[] args) throws Exception {
        if (method.equals("<init>")) {
            Object base = args[0];
            set(self, "field_78801_a", base == null ? 64 : num(get(base, "field_78090_t"), 64)); // textureWidth
            set(self, "field_78799_b", base == null ? 32 : num(get(base, "field_78089_u"), 32)); // textureHeight
            // ModelRenderer(model), (model, boxName) or (model, texOffX, texOffY)
            if (args.length == 2 && args[1] instanceof String) set(self, "field_78802_n", args[1]); // boxName
            if (args.length == 3) { set(self, "field_78803_o", args[1]); set(self, "field_78813_p", args[2]); }
            set(self, "field_78810_s", base); // baseModel
            set(self, "field_78804_l", new ArrayList<Object>()); // cubeList
            set(self, "field_78805_m", new ArrayList<Object>()); // childModels
            set(self, "field_78806_j", Boolean.TRUE); // showModel
            set(self, "field_78807_k", Boolean.FALSE); // isHidden
            set(self, "field_78809_i", Boolean.FALSE); // mirror
            if (base != null) list(base, "field_78092_r").add(self);
            return null;
        }
        if (method.equals("func_78793_a")) { // setRotationPoint
            set(self, "field_78800_c", args[0]);
            set(self, "field_78797_d", args[1]);
            set(self, "field_78798_e", args[2]);
            return null;
        }
        if (method.equals("func_78792_a")) { // addChild
            list(self, "field_78805_m").add(args[0]);
            return null;
        }
        if (method.equals("func_78786_a")) { // addBox(partName, ...): boxes carry the parent's name as a prefix
            Object name = get(self, "field_78802_n");
            set(self, "field_78802_n", (name == null ? "" : name + ".") + args[0]);
            return self;
        }
        if (method.equals("func_78790_a") || method.equals("func_78789_a")) { // addBox(x, y, z, w, h, d[, size])
            Object[] cube = new Object[] { get(self, "field_78803_o"), get(self, "field_78813_p"), get(self, "field_78809_i"), args };
            list(self, "field_78804_l").add(cube);
            return self;
        }
        if (method.equals("func_78784_a")) { // setTextureOffset
            set(self, "field_78803_o", args[0]);
            set(self, "field_78813_p", args[1]);
            return self;
        }
        if (method.equals("func_78787_b")) { // setTextureSize
            set(self, "field_78801_a", args[0]);
            set(self, "field_78799_b", args[1]);
            return self;
        }
        return null; // render / postRender: no geometry yet
    }

    private static Object manager(Object self, String method, Object[] args) throws Exception {
        if (method.equals("func_178631_a")) { // setPlayerViewY
            set(self, "field_178637_r", args[0]);
            return null;
        }
        if (method.equals("func_178633_a")) { // setRenderShadow
            set(self, "field_178638_s", args[0]);
            return null;
        }
        if (method.equals("getSkinMap")) { // Forge accessor for the player renderers by skin type
            Object map = get(self, "field_178636_l");
            if (map == null && self instanceof RenderManager) {
                Map<String, Object> created = new HashMap<String, Object>();
                created.put("default", new RenderPlayer((RenderManager) self, false));
                created.put("slim", new RenderPlayer((RenderManager) self, true));
                set(self, "field_178636_l", created);
                map = created;
            }
            return map;
        }
        return Boolean.FALSE; // renderEntityWithPosYaw: entity models are not drawn inside GUIs yet
    }

    /** The default skin is picked from the UUID hash, exactly like vanilla's DefaultPlayerSkin. */
    private static ResourceLocation skinFor(UUID id) {
        boolean slim = id != null && (id.hashCode() & 1) == 1;
        return new ResourceLocation("textures/entity/" + (slim ? "alex" : "steve") + ".png");
    }

    /** Renderer constructors' field setup (each stub constructor runs its super's with defaults first, then this). */
    private static Object construct(String cls, Object self, Object[] args) throws Exception {
        if (cls.equals("net.minecraft.client.renderer.entity.Render")) {
            if (args.length > 0 && args[0] != null) set(self, "field_76990_c", args[0]);
        } else if (cls.equals("net.minecraft.client.renderer.entity.RendererLivingEntity")) {
            if (args.length > 0 && args[0] != null) set(self, "field_76990_c", args[0]);
            if (args.length > 1 && args[1] != null) set(self, "field_77045_g", args[1]);
            if (args.length > 2 && args[2] != null) set(self, "field_76989_e", args[2]);
            list(self, "field_177097_h");
        } else if (cls.equals("net.minecraft.client.model.ModelBiped")) {
            // ModelBiped(), (size) and (size, yOffset, texW, texH) all end in the four-argument body.
            float size = args.length > 0 ? (Float) args[0] : 0F, y = args.length > 1 ? (Float) args[1] : 0F;
            biped(self, size, y, args.length > 2 ? (Integer) args[2] : 64, args.length > 3 ? (Integer) args[3] : 32);
        } else if (cls.equals("net.minecraft.client.model.ModelPlayer")) {
            // ModelPlayer(size, smallArms): super(size, 0, 64, 64) then the wear layers (right armwear's z of 10 is vanilla's).
            float s = (Float) args[0]; boolean slim = (Boolean) args[1];
            net.minecraft.client.model.ModelBase base = (net.minecraft.client.model.ModelBase) self;
            biped(self, s, 0F, 64, 64);
            set(self, "field_178735_y", slim);
            set(self, "field_178736_x", part(base, 24, 0, false, -3, -6, -1, 6, 6, 1, s, 0, 0, 0));
            net.minecraft.client.model.ModelRenderer cape = new net.minecraft.client.model.ModelRenderer(base, 0, 0);
            cape.func_78787_b(64, 32);
            cape.func_78790_a(-5, 0, -1, 10, 16, 1, s);
            set(self, "field_178729_w", cape);
            if (slim) {
                set(self, "field_178724_i", part(base, 32, 48, false, -1, -2, -2, 3, 12, 4, s, 5, 2.5F, 0));
                set(self, "field_178723_h", part(base, 40, 16, false, -2, -2, -2, 3, 12, 4, s, -5, 2.5F, 0));
                set(self, "field_178734_a", part(base, 48, 48, false, -1, -2, -2, 3, 12, 4, s + 0.25F, 5, 2.5F, 0));
                set(self, "field_178732_b", part(base, 40, 32, false, -2, -2, -2, 3, 12, 4, s + 0.25F, -5, 2.5F, 10));
            } else {
                set(self, "field_178724_i", part(base, 32, 48, false, -1, -2, -2, 4, 12, 4, s, 5, 2, 0));
                set(self, "field_178734_a", part(base, 48, 48, false, -1, -2, -2, 4, 12, 4, s + 0.25F, 5, 2, 0));
                set(self, "field_178732_b", part(base, 40, 32, false, -3, -2, -2, 4, 12, 4, s + 0.25F, -5, 2, 10));
            }
            set(self, "field_178722_k", part(base, 16, 48, false, -2, 0, -2, 4, 12, 4, s, 1.9F, 12, 0));
            set(self, "field_178733_c", part(base, 0, 48, false, -2, 0, -2, 4, 12, 4, s + 0.25F, 1.9F, 12, 0));
            set(self, "field_178731_d", part(base, 0, 32, false, -2, 0, -2, 4, 12, 4, s + 0.25F, -1.9F, 12, 0));
            set(self, "field_178730_v", part(base, 16, 32, false, -4, 0, -2, 8, 12, 4, s + 0.25F, 0, 0, 0));
        } else if (cls.equals("net.minecraft.client.renderer.entity.RenderPlayer")) {
            boolean slim = args.length > 1 && Boolean.TRUE.equals(args[1]);
            if (args.length > 0 && args[0] != null) set(self, "field_76990_c", args[0]);
            set(self, "field_177140_a", slim);
            set(self, "field_76989_e", 0.5F);
            net.minecraft.client.model.ModelPlayer model = new net.minecraft.client.model.ModelPlayer(0.0F, slim);
            set(self, "field_77045_g", model);
            RenderPlayer rp = (RenderPlayer) self;
            @SuppressWarnings("unchecked") List<Object> layers = (List<Object>) list(self, "field_177097_h");
            layers.clear();
            layers.add(new net.minecraft.client.renderer.entity.layers.LayerBipedArmor(rp));
            layers.add(new net.minecraft.client.renderer.entity.layers.LayerHeldItem(rp));
            layers.add(new net.minecraft.client.renderer.entity.layers.LayerArrow(rp));
            layers.add(new net.minecraft.client.renderer.entity.layers.LayerDeadmau5Head(rp));
            layers.add(new net.minecraft.client.renderer.entity.layers.LayerCape(rp));
            layers.add(new net.minecraft.client.renderer.entity.layers.LayerCustomHead(model.field_78116_c));
        }
        return null;
    }

    /** ModelBiped's four-argument constructor body. */
    private static void biped(Object self, float size, float y, int w, int h) throws Exception {
        set(self, "field_78090_t", w);
        set(self, "field_78089_u", h);
        net.minecraft.client.model.ModelBase base = (net.minecraft.client.model.ModelBase) self;
        set(self, "field_78116_c", part(base, 0, 0, false, -4, -8, -4, 8, 8, 8, size, 0, y, 0)); // bipedHead
        set(self, "field_178720_f", part(base, 32, 0, false, -4, -8, -4, 8, 8, 8, size + 0.5F, 0, y, 0)); // bipedHeadwear
        set(self, "field_78115_e", part(base, 16, 16, false, -4, 0, -2, 8, 12, 4, size, 0, y, 0)); // bipedBody
        set(self, "field_178723_h", part(base, 40, 16, false, -3, -2, -2, 4, 12, 4, size, -5, 2 + y, 0)); // bipedRightArm
        set(self, "field_178724_i", part(base, 40, 16, true, -1, -2, -2, 4, 12, 4, size, 5, 2 + y, 0)); // bipedLeftArm
        set(self, "field_178721_j", part(base, 0, 16, false, -2, 0, -2, 4, 12, 4, size, -1.9F, 12 + y, 0)); // bipedRightLeg
        set(self, "field_178722_k", part(base, 0, 16, true, -2, 0, -2, 4, 12, 4, size, 1.9F, 12 + y, 0)); // bipedLeftLeg
    }

    /** `new ModelRenderer(base, u, v)`, `mirror`, `addBox(x, y, z, w, h, d, size)`, `setRotationPoint(px, py, pz)`. */
    private static Object part(net.minecraft.client.model.ModelBase base, int u, int v, boolean mirror, float x, float y, float z,
                               int w, int h, int d, float size, float px, float py, float pz) {
        net.minecraft.client.model.ModelRenderer r = new net.minecraft.client.model.ModelRenderer(base, u, v);
        r.field_78809_i = mirror;
        r.func_78790_a(x, y, z, w, h, d, size);
        r.func_78793_a(px, py, pz);
        return r;
    }

    /** Nothing is downloaded, so the callback sees the default skin (the first Type constant is SKIN). */
    private static void skinAvailable(Object[] args) throws Exception {
        Object callback = args.length > 1 ? args[1] : null;
        if (callback == null || !(args[0] instanceof GameProfile)) return;
        GameProfile profile = (GameProfile) args[0];
        UUID id = profile.getId();
        if (id == null) {
            id = UUID.nameUUIDFromBytes(("OfflinePlayer:" + profile.getName()).getBytes(StandardCharsets.UTF_8));
        }
        ResourceLocation skin = skinFor(id);
        for (Method m : callback.getClass().getMethods()) {
            if (!m.getName().equals("func_180521_a")) continue; // SkinAvailableCallback.skinAvailable
            m.setAccessible(true);
            m.invoke(callback, m.getParameterTypes()[0].getEnumConstants()[0], skin, null);
            return;
        }
    }

    private static Object get(Object object, String name) throws Exception {
        if (object == null) return null;
        try {
            return field(object.getClass(), name).get(object);
        } catch (NoSuchFieldException e) {
            return null;
        }
    }

    private static void set(Object object, String name, Object value) {
        if (object == null) return;
        try {
            field(object.getClass(), name).set(object, value);
        } catch (Exception e) {
            // A missing SRG field only costs this piece of mirrored state.
        }
    }

    @SuppressWarnings("unchecked")
    private static List<Object> list(Object object, String name) {
        Object value = null;
        try {
            value = get(object, name);
        } catch (Exception e) {
            // fall through to a fresh list
        }
        if (value instanceof List) return (List<Object>) value;
        List<Object> created = new ArrayList<Object>();
        set(object, name, created);
        return created;
    }

    private static int num(Object value, int fallback) {
        return value instanceof Number ? ((Number) value).intValue() : fallback;
    }

    private static Field field(Class<?> type, String name) throws NoSuchFieldException {
        Field cached = FIELDS.get(type.getName() + "." + name);
        if (cached != null) return cached;
        for (Class<?> c = type; c != null; c = c.getSuperclass()) { // SRG fields sit on the vanilla superclass
            try {
                Field f = c.getDeclaredField(name);
                f.setAccessible(true);
                FIELDS.put(type.getName() + "." + name, f);
                return f;
            } catch (NoSuchFieldException e) {
                // keep walking up
            }
        }
        throw new NoSuchFieldException(name);
    }
}
