package rustmc.forge;

import com.google.gson.*;
import com.mojang.authlib.GameProfile;
import io.netty.buffer.ByteBuf;
import io.netty.buffer.Unpooled;
import java.lang.reflect.*;
import java.util.*;
import java.util.concurrent.ConcurrentLinkedQueue;
import net.minecraft.client.Minecraft;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.client.multiplayer.WorldClient;
import net.minecraft.entity.*;
import net.minecraft.entity.player.*;
import net.minecraft.item.*;
import net.minecraft.network.*;
import net.minecraft.util.*;
import net.minecraftforge.event.entity.EntityJoinWorldEvent;
import net.minecraftforge.event.world.WorldEvent;

/** Stable, Rust-owned views. Constructors that would start a second game are never run.
 * All mutation of the live object graph is on the Rust game thread. Actions from Java
 * worker threads are queued and consumed at the next boundary on that thread. */
public final class Live {
    private static final Gson JSON = new Gson();
    private static final Map<Integer, Entity> ENTITIES = new LinkedHashMap<>();
    private static final Map<Integer, Item> ITEMS = new HashMap<>();
    private static final IdentityHashMap<Item, Integer> ITEM_IDS = new IdentityHashMap<>();
    private static final Map<String, short[]> SECTIONS = new HashMap<>();
    private static final Map<Long, byte[]> BIOMES = new HashMap<>();
    /** Per-section light, `sky<<4|block` indexed like SECTIONS; the world's skylightSubtracted and provider.hasNoSky. */
    private static final Map<String, byte[]> LIGHT = new HashMap<>();
    private static int skylightSubtracted;
    /** Chunk objects handed out by getChunkFromChunkCoords, one per column; their methods read the mirror. */
    private static final Map<Long, Object> CHUNKS = new HashMap<>();
    private static boolean noSky;
    private static final Map<ItemStack, String> TAGS = new WeakHashMap<>();
    private static final Map<Class<?>, Map<String, Field>> FIELDS = new HashMap<>();
    private static final ConcurrentLinkedQueue<JsonObject> ACTIONS = new ConcurrentLinkedQueue<>();
    private static long generation = Long.MIN_VALUE, time, totalTime;
    private static WorldClient world;
    private static EntityPlayerSP player;
    private static JsonObject baseline = new JsonObject();

    @SuppressWarnings("unchecked")
    private static <T> T allocate(Class<T> type) {
        return (T) Stub.call("rustmc.allocate", null, new Object[] {type});
    }
    private static Object allocate(String name) throws Exception { return allocate(Class.forName(name, true, Live.class.getClassLoader())); }
    private static Field field(Class<?> type, String name) throws Exception {
        Map<String, Field> cache = FIELDS.get(type);
        if (cache == null) { cache = new HashMap<>(); FIELDS.put(type, cache); }
        Field f = cache.get(name);
        if (f != null) return f;
        for (Class<?> c = type; c != null; c = c.getSuperclass()) {
            try { f = c.getDeclaredField(name); f.setAccessible(true); cache.put(name, f); return f; }
            catch (NoSuchFieldException absent) { /* look at the declaring superclass */ }
        }
        throw new NoSuchFieldException(type.getName() + "." + name);
    }
    private static void set(Object object, String name, Object value) throws Exception { field(object.getClass(), name).set(object, value); }
    private static Object get(Object object, String name) throws Exception { return field(object.getClass(), name).get(object); }
    private static double number(JsonObject o, String key) { return o.get(key).getAsDouble(); }
    private static int integer(JsonObject o, String key) { return o.get(key).getAsInt(); }
    private static boolean flag(JsonObject o, String key) { return o.get(key).getAsBoolean(); }
    private static String string(JsonObject o, String key) { return o.get(key).getAsString(); }
    private static UUID uuid(String s, int id) {
        try { return UUID.fromString(s.replaceFirst("^(\\w{8})(\\w{4})(\\w{4})(\\w{4})(\\w{12})$", "$1-$2-$3-$4-$5")); }
        catch (IllegalArgumentException ignored) { return new UUID(0, id); }
    }

    public static void sync(String text) throws Exception {
        JsonObject root = new JsonParser().parse(text).getAsJsonObject();
        Entities.state(root);
        Minecraft mc = Minecraft.func_71410_x();
        long next = root.get("generation").getAsLong();
        boolean active = flag(root, "active");
        if (next != generation || !active) {
            if (world != null) Bus.post(new WorldEvent.Unload(world));
            ENTITIES.clear(); SECTIONS.clear(); BIOMES.clear(); LIGHT.clear(); CHUNKS.clear(); ACTIONS.clear(); player = null;
            world = null; generation = next;
        }
        if (!active) {
            set(mc, "field_71439_g", null); set(mc, "field_71441_e", null);
            set(mc, "field_175622_Z", null); set(mc, "field_147125_j", null); set(mc, "field_71476_x", null);
            return;
        }
        boolean loaded = world == null;
        if (loaded) {
            world = allocate(WorldClient.class);
            set(world, "field_72996_f", new ArrayList<Entity>());
            set(world, "field_73010_i", new ArrayList<EntityPlayer>());
            set(world, "field_73012_v", new Random(0));
            set(world, "field_72995_K", true);
        }
        set(mc, "field_71441_e", world);
        if (loaded) Bus.post(new WorldEvent.Load(world));
        List<Entity> joined = new ArrayList<>();
        time = root.get("time").getAsLong(); totalTime = root.get("total_time").getAsLong();
        Set<Integer> seen = new HashSet<>();
        for (JsonElement row : root.getAsJsonArray("entities")) {
            JsonObject data = row.getAsJsonObject(); int id = integer(data, "id"); seen.add(id);
            String type = string(data, "class");
            Entity e = ENTITIES.get(id);
            if (e == null || !e.getClass().getName().equals(type)) {
                e = (Entity) allocate(type); ENTITIES.put(id, e); joined.add(e);
                set(e, "field_70180_af", new DataWatcher(e));
                ((DataWatcher)get(e,"field_70180_af")).func_75682_a(0, Byte.valueOf((byte) 0));
                ((DataWatcher)get(e,"field_70180_af")).func_75682_a(1, Short.valueOf((short) 300));
                if (e instanceof EntityLivingBase) ((DataWatcher)get(e,"field_70180_af")).func_75682_a(6, Float.valueOf(20));
                if (e instanceof EntityPlayer) {
                    EntityPlayer p = (EntityPlayer) e;
                    set(p, "field_146106_i", new GameProfile(uuid(string(data, "uuid"), id), string(data, "name")));
                    InventoryPlayer inv = allocate(InventoryPlayer.class);
                    inv.field_70462_a = new ItemStack[36]; inv.field_70460_b = new ItemStack[4];
                    set(inv, "field_70458_d", p); set(p, "field_71071_by", inv);
                    set(p, "field_71075_bZ", allocate(PlayerCapabilities.class));
                }
                if (e instanceof EntityPlayerSP) {
                    set(e, "field_71158_b", allocate(MovementInput.class));
                    set(e, "field_71174_a", allocate("net.minecraft.client.network.NetHandlerPlayClient"));
                }
            }
            set(e, "field_145783_c", id); set(e, "field_70170_p", world);
            String[] ds = {"field_70165_t", "field_70163_u", "field_70161_v", "field_70169_q", "field_70167_r", "field_70166_s", "field_70159_w", "field_70181_x", "field_70179_y", "field_70142_S", "field_70137_T", "field_70136_U"};
            String[] keys = {"x", "y", "z", "prev_x", "prev_y", "prev_z", "motion_x", "motion_y", "motion_z", "last_x", "last_y", "last_z"};
            for (int i = 0; i < ds.length; i++) set(e, ds[i], number(data, keys[i]));
            set(e, "field_70177_z", (float) number(data, "yaw")); set(e, "field_70125_A", (float) number(data, "pitch"));
            set(e, "field_70126_B", (float) number(data, "prev_yaw")); set(e, "field_70127_C", (float) number(data, "prev_pitch"));
            set(e, "field_70122_E", flag(data, "on_ground")); set(e, "field_70128_L", flag(data, "dead"));
            set(e, "field_70145_X", flag(data, "no_clip")); set(e, "field_70173_aa", integer(data, "ticks"));
            set(e, "field_70130_N", (float) number(data, "width")); set(e, "field_70131_O", (float) number(data, "height"));
            set(e, "field_70143_R", (float) number(data, "fall_distance"));
            double w = number(data, "width") / 2, x = e.field_70165_t, y = e.field_70163_u, z = e.field_70161_v;
            set(e, "field_70121_D", new AxisAlignedBB(x-w, y, z-w, x+w, y+number(data,"height"), z+w));
            ((DataWatcher)get(e,"field_70180_af")).func_75692_b(0, Byte.valueOf((byte) integer(data, "flags")));
            if (e instanceof EntityLivingBase) {
                ((DataWatcher)get(e,"field_70180_af")).func_75692_b(6, Float.valueOf((float) number(data, "health")));
                set(e, "field_70737_aN", integer(data, "hurt_time"));
                set(e, "field_70759_as", (float) number(data, "yaw"));
            }
            if (e instanceof EntityPlayerSP) {
                player = (EntityPlayerSP)e;
                set(mc, "field_71439_g", player);
                player.field_71071_by.field_70461_c = integer(root, "selected_slot");
                if(get(mc,"field_71442_b")==null) set(mc, "field_71442_b", allocate("net.minecraft.client.multiplayer.PlayerControllerMP"));
            }
        }
        ENTITIES.keySet().retainAll(seen);
        @SuppressWarnings("unchecked") List<Entity> entities = (List<Entity>) get(world, "field_72996_f");
        entities.clear(); entities.addAll(ENTITIES.values());
        @SuppressWarnings("unchecked") List<EntityPlayer> players = (List<EntityPlayer>) get(world, "field_73010_i");
        players.clear(); for (Entity e : entities) if (e instanceof EntityPlayer) players.add((EntityPlayer)e);
        // Rust owns spawning, so cancelling EntityJoinWorldEvent cannot veto it; the event is informational.
        for (Entity e : joined) Bus.post(new EntityJoinWorldEvent(e, world));
        if (root.has("view")) view(mc, root.getAsJsonObject("view"));
        if (root.has("tab")) Tab.sync(mc, world, root.getAsJsonObject("tab"));
        if (player != null) {
            set(player, "field_71093_bK", root.get("dimension").getAsInt());
            noSky = root.get("dimension").getAsInt() == -1;
            JsonObject caps = root.getAsJsonObject("capabilities"); PlayerCapabilities c = player.field_71075_bZ;
            set(c,"field_75100_b",flag(caps,"flying")); set(c,"field_75101_c",flag(caps,"allow_flying"));
            set(c,"field_75098_d",flag(caps,"creative")); set(c,"field_75102_a",flag(caps,"invulnerable"));
            JsonObject input = root.getAsJsonObject("input"); MovementInput movement = player.field_71158_b;
            movement.field_78902_a = (float) number(input,"strafe"); movement.field_78900_b = (float) number(input,"forward");
            movement.field_78901_c = flag(input,"jump"); movement.field_78899_d = flag(input,"sneak");
            JsonArray inv = root.getAsJsonArray("inventory");
            for (int i=0;i<40;i++) { ItemStack old=i<36?player.field_71071_by.field_70462_a[i]:player.field_71071_by.field_70460_b[i-36]; ItemStack s = stack(inv.get(i),old); if(i<36) player.field_71071_by.field_70462_a[i]=s; else player.field_71071_by.field_70460_b[i-36]=s; }
        }
        JsonObject settings = root.getAsJsonObject("settings"); Object options = get(mc,"field_71474_y");
        set(options,"field_74320_O",integer(settings,"third_person")); set(options,"field_74334_X",(float)number(settings,"fov"));
        set(options,"field_74341_c",(float)number(settings,"sensitivity")); set(options,"field_151451_c",integer(settings,"render_distance"));
        if(settings.has("keys")) keys(options,settings.getAsJsonObject("keys"));
        Object timer = get(mc,"field_71428_T");
        if(timer==null) { timer=allocate("net.minecraft.util.Timer"); set(mc,"field_71428_T",timer); }
        set(timer,"field_74278_d",1.0f); set(timer,"field_74281_c",(float)number(root,"partial_ticks"));
        baseline = playerState();
    }

    /** RenderGlobal as mods read it reflectively: renderInfos sized to the drawn sections, viewFrustum to the loaded ones. */
    private static void renderGlobal(Minecraft mc, JsonObject v) throws Exception {
        Object rg = get(mc, "field_71438_f");
        if (rg == null) {
            rg = allocate("net.minecraft.client.renderer.RenderGlobal"); set(mc, "field_71438_f", rg);
            set(rg, "field_72755_R", new ArrayList<Object>());
            set(rg, "field_72777_q", mc); set(rg, "field_175010_j", get(mc, "field_175616_W"));
        }
        set(rg, "field_72769_h", world);
        Object gs = get(mc, "field_71474_y");
        if (gs != null) set(rg, "field_72739_F", get(gs, "field_151451_c")); // renderDistanceChunks
        int drawn = v.has("sections_drawn") ? integer(v, "sections_drawn") : 0;
        int total = v.has("sections_total") ? integer(v, "sections_total") : 0;
        @SuppressWarnings("unchecked") List<Object> infos = (List<Object>) get(rg, "field_72755_R");
        // Mods only count these; the entries stand in for the real ContainerLocalRenderInformation rows.
        while (infos.size() > drawn) infos.remove(infos.size() - 1);
        while (infos.size() < drawn) infos.add(Boolean.TRUE);
        Object frustum = get(rg, "field_175008_n");
        Class<?> chunk = Class.forName("net.minecraft.client.renderer.chunk.RenderChunk", false, Live.class.getClassLoader());
        Object[] chunks = frustum == null ? null : (Object[]) get(frustum, "field_178164_f");
        if (frustum == null) { frustum = allocate("net.minecraft.client.renderer.ViewFrustum"); set(rg, "field_175008_n", frustum); }
        if (chunks == null || chunks.length != total) set(frustum, "field_178164_f", java.lang.reflect.Array.newInstance(chunk, total));
    }

    /** Minecraft.renderManager exists from startGame on (mods add layers in init); the camera fields follow `view`. */
    /** Minecraft.timer exists from startup (vanilla builds it in the constructor): mods read it before any world sync. */
    static void timer(Minecraft mc) throws Exception {
        if(get(mc,"field_71428_T")==null) {
            Object timer=allocate("net.minecraft.util.Timer");
            set(timer,"field_74278_d",1.0f); set(timer,"field_74282_a",20.0f);
            set(mc,"field_71428_T",timer);
        }
        // ingameGUI: its getChatGUI is a Rust handler, but mods read the field first (LunarForge's HypixelLocation ticks).
        if(get(mc,"field_71456_v")==null) set(mc,"field_71456_v",allocate("net.minecraft.client.gui.GuiIngame"));
        // entityRenderer: its methods are Rust handlers (shaders, fov, lightmap); mods dereference the field directly.
        if(get(mc,"field_71460_t")==null) set(mc,"field_71460_t",allocate("net.minecraft.client.renderer.EntityRenderer"));
    }

    static Object renderManager(Minecraft mc) throws Exception {
        Object rm = get(mc, "field_175616_W");
        if (rm == null) { rm = allocate("net.minecraft.client.renderer.entity.RenderManager"); set(mc, "field_175616_W", rm); }
        return rm;
    }

    /** The camera as RenderManager reports it (interpolated feet, as vanilla's renderPos) and what the crosshair hits. */
    private static void view(Minecraft mc, JsonObject v) throws Exception {
        Object rm = renderManager(mc);
        double x = number(v, "x"), y = number(v, "y"), z = number(v, "z");
        for (String f : new String[] {"field_78730_l", "field_78725_b"}) set(rm, f, x);
        for (String f : new String[] {"field_78731_m", "field_78726_c"}) set(rm, f, y);
        for (String f : new String[] {"field_78728_n", "field_78723_d"}) set(rm, f, z);
        set(rm, "field_78735_i", (float) number(v, "yaw")); set(rm, "field_78732_j", (float) number(v, "pitch"));
        set(rm, "field_78734_h", player); set(rm, "field_78722_g", world); set(rm, "field_78733_k", get(mc, "field_71474_y"));
        set(mc, "field_175622_Z", player);
        renderGlobal(mc, v);
        JsonObject t = v.getAsJsonObject("target");
        String kind = string(t, "type");
        Entity hit = kind.equals("entity") ? ENTITIES.get(integer(t, "id")) : null;
        set(mc, "field_147125_j", hit);
        if (kind.equals("miss") || (kind.equals("entity") && hit == null)) { set(mc, "field_71476_x", null); return; }
        Object mop = allocate("net.minecraft.util.MovingObjectPosition");
        Class<?> types = Class.forName("net.minecraft.util.MovingObjectPosition$MovingObjectType", true, Live.class.getClassLoader());
        set(mop, "field_72313_a", types.getEnumConstants()[kind.equals("entity") ? 2 : 1]);
        set(mop, "field_72307_f", new Vec3(number(t, "hx"), number(t, "hy"), number(t, "hz")));
        set(mop, "field_72308_g", hit);
        if (kind.equals("block")) {
            set(mop, "field_178783_e", new BlockPos(integer(t, "x"), integer(t, "y"), integer(t, "z")));
            set(mop, "field_178784_b", EnumFacing.values()[integer(t, "side")]);
        }
        set(mc, "field_71476_x", mop);
    }

    /** GameSettings' vanilla bindings: field, description, category. Codes come from the client's controls. */
    private static final String[][] BINDINGS = {
        {"field_74312_F","key.attack","key.categories.gameplay"},{"field_74313_G","key.use","key.categories.gameplay"},
        {"field_74351_w","key.forward","key.categories.movement"},{"field_74370_x","key.left","key.categories.movement"},
        {"field_74368_y","key.back","key.categories.movement"},{"field_74366_z","key.right","key.categories.movement"},
        {"field_74314_A","key.jump","key.categories.movement"},{"field_74311_E","key.sneak","key.categories.movement"},
        {"field_151444_V","key.sprint","key.categories.movement"},{"field_74316_C","key.drop","key.categories.gameplay"},
        {"field_151445_Q","key.inventory","key.categories.inventory"},{"field_74310_D","key.chat","key.categories.multiplayer"},
        {"field_74321_H","key.playerlist","key.categories.multiplayer"},{"field_74322_I","key.pickItem","key.categories.gameplay"},
        {"field_74323_J","key.command","key.categories.multiplayer"},{"field_151447_Z","key.screenshot","key.categories.misc"},
        {"field_151457_aa","key.togglePerspective","key.categories.misc"},{"field_151458_ab","key.smoothCamera","key.categories.misc"},
        {"field_152396_an","key.streamStartStop","key.categories.stream"},{"field_152397_ao","key.streamPauseUnpause","key.categories.stream"},
        {"field_152398_ap","key.streamCommercial","key.categories.stream"},{"field_152399_aq","key.streamToggleMic","key.categories.stream"},
        {"field_152395_am","key.fullscreen","key.categories.misc"},{"field_178883_an","key.spectatorOutlines","key.categories.misc"}};
    private static final List<net.minecraft.client.settings.KeyBinding> VANILLA_KEYS = new ArrayList<>();
    /** GameSettings' default key codes (hotbar N is N+1), for the bindings created before the first snapshot. */
    private static final Map<String,Integer> DEFAULT_KEYS = new HashMap<>();
    static { Object[] d={"key.attack",-100,"key.use",-99,"key.forward",17,"key.left",30,"key.back",31,"key.right",32,"key.jump",57,"key.sneak",42,"key.sprint",29,"key.drop",16,"key.inventory",18,"key.chat",20,"key.playerlist",15,"key.pickItem",-98,"key.command",53,"key.screenshot",60,"key.togglePerspective",63,"key.smoothCamera",0,"key.streamStartStop",64,"key.streamPauseUnpause",65,"key.streamCommercial",0,"key.streamToggleMic",0,"key.fullscreen",87,"key.spectatorOutlines",0}; for(int i=0;i<d.length;i+=2) DEFAULT_KEYS.put((String)d[i],(Integer)d[i+1]); for(int i=1;i<=9;i++) DEFAULT_KEYS.put("key.hotbar."+i,i+1); }
    private static int code(JsonObject keys,String desc) {
        if(keys!=null&&keys.has(desc)) return keys.get(desc).getAsInt();
        Integer d=DEFAULT_KEYS.get(desc); return d==null?0:d;
    }
    /** GameSettings' constructor creates every binding: Minecraft's setup calls this so mods never see a null one. */
    public static void defaultKeys(Object options) throws Exception { keys(options,null); }
    /** Created once (their constructors register them with the Rust key table), then rebound when controls change. */
    private static void keys(Object options,JsonObject keys) throws Exception {
        if(VANILLA_KEYS.isEmpty()) {
            List<net.minecraft.client.settings.KeyBinding> all=new ArrayList<>();
            for(String[] b:BINDINGS) { net.minecraft.client.settings.KeyBinding k=new net.minecraft.client.settings.KeyBinding(b[1],code(keys,b[1]),b[2]); set(options,b[0],k); all.add(k); }
            net.minecraft.client.settings.KeyBinding[] hotbar=new net.minecraft.client.settings.KeyBinding[9];
            for(int i=0;i<9;i++) { hotbar[i]=new net.minecraft.client.settings.KeyBinding("key.hotbar."+(i+1),code(keys,"key.hotbar."+(i+1)),"key.categories.inventory"); all.add(hotbar[i]); }
            set(options,"field_151456_ac",hotbar);
            VANILLA_KEYS.addAll(all);
            // Mods (and Forge's own registerKeyBinding) append to this array.
            Object existing=get(options,"field_74324_K");
            if(existing instanceof net.minecraft.client.settings.KeyBinding[]) for(net.minecraft.client.settings.KeyBinding k:(net.minecraft.client.settings.KeyBinding[])existing) if(k!=null&&!all.contains(k)) all.add(k);
            set(options,"field_74324_K",all.toArray(new net.minecraft.client.settings.KeyBinding[0]));
        }
        for(net.minecraft.client.settings.KeyBinding k:VANILLA_KEYS) { int c=code(keys,(String)get(k,"field_74515_c")); if((Integer)get(k,"field_74512_d")!=c) set(k,"field_74512_d",c); }
    }

    /** The Java view of a Rust entity id, for events that name an entity. */
    public static Entity entity(int id) { return ENTITIES.get(id); }

    static ItemStack stack(JsonElement row, ItemStack existing) throws Exception {
        if(row.isJsonNull()) return null;
        JsonObject o=row.getAsJsonObject(); int id=integer(o,"id");
        // The vanilla registry's own Item (ModLoader.registries), so `stack.getItem() == Items.x` holds for mods.
        Item item=Item.func_150899_d(id);
        if(item==null) item=ITEMS.computeIfAbsent(id,k->{ try { Item i=(Item)allocate(string(o,"class")); ITEM_IDS.put(i,k); return i; } catch(Exception e) { throw new RuntimeException(e); } });
        // The real constructor: Forge's getItem reads the registry delegate it sets, and it attaches capabilities.
        ItemStack stack=existing!=null&&existing.func_77973_b()==item?existing:new ItemStack(item,0,0);
        set(stack,"field_77994_a",integer(o,"count")); set(stack,"field_77991_e",integer(o,"damage"));
        // stackTagCompound from the client's NBT bytes (an uncompressed named root), parsed only when they change.
        JsonElement nbt=o.get("nbt");
        String key=nbt==null||nbt.isJsonNull()?null:nbt.toString();
        if(!Objects.equals(TAGS.get(stack),key)||(key==null&&stack.func_77978_p()!=null)) {
            if(key==null) stack.func_77982_d(null);
            else {
                JsonArray a=nbt.getAsJsonArray(); byte[] b=new byte[a.size()];
                for(int i=0;i<b.length;i++) b[i]=(byte)a.get(i).getAsInt();
                try { stack.func_77982_d(net.minecraft.nbt.CompressedStreamTools.func_74794_a(new java.io.DataInputStream(new java.io.ByteArrayInputStream(b)))); }
                catch(java.io.IOException e) { stack.func_77982_d(null); }
            }
            TAGS.put(stack,key);
        }
        return stack;
    }

    /** Sparse section copies are updated only when Rust dirties a section. */
    public static void section(int x,int y,int z,short[] data) {
        String key=x+","+y+","+z;
        if(data.length==0) SECTIONS.remove(key); else SECTIONS.put(key,data.clone());
    }

    /** A section's packed light (`sky<<4|block`); an empty array drops it. */
    public static void light(int x,int y,int z,byte[] data) {
        String key=x+","+y+","+z;
        if(data.length==0) LIGHT.remove(key); else LIGHT.put(key,data.clone());
    }
    public static void skylightSubtracted(int v) { skylightSubtracted=v; }

    private static boolean loaded(BlockPos pos) {
        return BIOMES.containsKey((long)(pos.func_177958_n()>>4)<<32|((pos.func_177952_p()>>4)&0xffffffffL));
    }
    private static boolean valid(BlockPos p) {
        return p.func_177958_n()>=-30000000&&p.func_177952_p()>=-30000000&&p.func_177958_n()<30000000&&p.func_177952_p()<30000000&&p.func_177956_o()>=0&&p.func_177956_o()<256;
    }
    /** Chunk.canSeeSky over the mirror: no loaded section with blocks above the position's section. */
    private static boolean openAbove(BlockPos p) {
        int x=p.func_177958_n()>>4, z=p.func_177952_p()>>4;
        for(int cy=(p.func_177956_o()>>4)+1;cy<16;cy++) if(SECTIONS.containsKey(x+","+cy+","+z)) return false;
        return true;
    }
    private static byte[] lightSection(BlockPos p) {
        return LIGHT.get((p.func_177958_n()>>4)+","+(p.func_177956_o()>>4)+","+(p.func_177952_p()>>4));
    }
    private static int lightIndex(BlockPos p) {
        return (p.func_177956_o()&15)<<8|(p.func_177952_p()&15)<<4|(p.func_177958_n()&15);
    }
    /** Chunk.getLightFor: the stored value, else (no section) sky default where the column is open, 0 otherwise. */
    private static int chunkLight(boolean sky, BlockPos p) {
        byte[] l=lightSection(p);
        if(l==null) return sky&&openAbove(p)?15:0;
        int v=l[lightIndex(p)]&255;
        return sky?(noSky?0:v>>4):v&15;
    }
    /** World.getLightFor(type, pos). */
    public static int lightFor(boolean sky, BlockPos p) {
        if(p.func_177956_o()<0) p=new BlockPos(p.func_177958_n(),0,p.func_177952_p());
        if(!valid(p)||!loaded(p)) return sky?15:0;
        return chunkLight(sky,p);
    }
    private static boolean neighbourLit(BlockPos p) {
        return ((net.minecraft.block.state.IBlockState)blockState(p)).func_177230_c().func_149710_n();
    }
    /** World.getLightFromNeighborsFor(type, pos). */
    public static int lightFromNeighborsFor(boolean sky, BlockPos p) {
        if(noSky&&sky) return 0;
        if(p.func_177956_o()<0) p=new BlockPos(p.func_177958_n(),0,p.func_177952_p());
        if(!valid(p)||!loaded(p)) return sky?15:0;
        if(neighbourLit(p)) return Math.max(lightFor(sky,p.func_177984_a()),Math.max(Math.max(lightFor(sky,p.func_177974_f()),lightFor(sky,p.func_177976_e())),Math.max(lightFor(sky,p.func_177968_d()),lightFor(sky,p.func_177978_c()))));
        return chunkLight(sky,p);
    }
    /** Chunk.getLightSubtracted(pos, amount). */
    public static int lightSubtracted(BlockPos p,int amount) {
        byte[] l=lightSection(p);
        if(l==null) return !noSky&&amount<15?15-amount:0;
        int v=l[lightIndex(p)]&255;
        return Math.max((noSky?0:v>>4)-amount,v&15);
    }
    /** World.getLight(pos, checkNeighbors). */
    public static int light(BlockPos p,boolean neighbours) {
        if(!(p.func_177958_n()>=-30000000&&p.func_177952_p()>=-30000000&&p.func_177958_n()<30000000&&p.func_177952_p()<30000000)) return 15;
        if(neighbours&&neighbourLit(p)) return Math.max(light(p.func_177984_a(),false),Math.max(Math.max(light(p.func_177974_f(),false),light(p.func_177976_e(),false)),Math.max(light(p.func_177968_d(),false),light(p.func_177978_c(),false))));
        if(p.func_177956_o()<0) return 0;
        if(p.func_177956_o()>=256) p=new BlockPos(p.func_177958_n(),255,p.func_177952_p());
        return lightSubtracted(p,skylightSubtracted);
    }
    /**
     * Entity.getBrightness / getBrightnessForRender at the eye (EntityFX: at its position): packed combined light
     * when `combined`, else the brightness-table value; 0 when the column is not loaded.
     */
    public static Object entityLight(Object entity,boolean combined,boolean eye) throws Exception {
        double x=((Number)get(entity,"field_70165_t")).doubleValue(), y=((Number)get(entity,"field_70163_u")).doubleValue(), z=((Number)get(entity,"field_70161_v")).doubleValue();
        if(eye) y+=((Entity)entity).func_70047_e();
        BlockPos p=new BlockPos(x,y,z);
        if(combined) return loaded(p)?lightFromNeighborsFor(true,p)<<20|lightFromNeighborsFor(false,p)<<4:0;
        return loaded(p)?brightness(light(p,true)):0.0F;
    }

    /**
     * World.rayTraceBlocks(start, end, stopOnLiquid, ignoreBlockWithoutBoundingBox, returnLastUncollidableBlock),
     * vanilla's DDA walk over the mirrored states; the real Block classes answer collision and the hit itself.
     */
    public static Object rayTrace(net.minecraft.world.World w, net.minecraft.util.Vec3 a, net.minecraft.util.Vec3 b, boolean liquid, boolean ignoreNoBox, boolean returnLast) {
        if(Double.isNaN(a.field_72450_a)||Double.isNaN(a.field_72448_b)||Double.isNaN(a.field_72449_c)) return null;
        if(Double.isNaN(b.field_72450_a)||Double.isNaN(b.field_72448_b)||Double.isNaN(b.field_72449_c)) return null;
        int i=net.minecraft.util.MathHelper.func_76128_c(b.field_72450_a), j=net.minecraft.util.MathHelper.func_76128_c(b.field_72448_b), k=net.minecraft.util.MathHelper.func_76128_c(b.field_72449_c);
        int l=net.minecraft.util.MathHelper.func_76128_c(a.field_72450_a), i1=net.minecraft.util.MathHelper.func_76128_c(a.field_72448_b), j1=net.minecraft.util.MathHelper.func_76128_c(a.field_72449_c);
        BlockPos pos=new BlockPos(l,i1,j1);
        net.minecraft.block.state.IBlockState st=(net.minecraft.block.state.IBlockState)blockState(pos);
        net.minecraft.block.Block block=st.func_177230_c();
        if((!ignoreNoBox||block.func_180640_a(w,pos,st)!=null)&&block.func_176209_a(st,liquid)) {
            net.minecraft.util.MovingObjectPosition hit=block.func_180636_a(w,pos,a,b);
            if(hit!=null) return hit;
        }
        net.minecraft.util.MovingObjectPosition last=null;
        int n=200;
        while(n-->=0) {
            if(Double.isNaN(a.field_72450_a)||Double.isNaN(a.field_72448_b)||Double.isNaN(a.field_72449_c)) return null;
            if(l==i&&i1==j&&j1==k) return returnLast?last:null;
            boolean fx=true, fy=true, fz=true;
            double d0=999.0D, d1=999.0D, d2=999.0D;
            if(i>l) d0=(double)l+1.0D; else if(i<l) d0=(double)l+0.0D; else fx=false;
            if(j>i1) d1=(double)i1+1.0D; else if(j<i1) d1=(double)i1+0.0D; else fy=false;
            if(k>j1) d2=(double)j1+1.0D; else if(k<j1) d2=(double)j1+0.0D; else fz=false;
            double d3=999.0D, d4=999.0D, d5=999.0D;
            double d6=b.field_72450_a-a.field_72450_a, d7=b.field_72448_b-a.field_72448_b, d8=b.field_72449_c-a.field_72449_c;
            if(fx) d3=(d0-a.field_72450_a)/d6;
            if(fy) d4=(d1-a.field_72448_b)/d7;
            if(fz) d5=(d2-a.field_72449_c)/d8;
            if(d3==-0.0D) d3=-1.0E-4D;
            if(d4==-0.0D) d4=-1.0E-4D;
            if(d5==-0.0D) d5=-1.0E-4D;
            net.minecraft.util.EnumFacing face;
            if(d3<d4&&d3<d5) { face=i>l?net.minecraft.util.EnumFacing.WEST:net.minecraft.util.EnumFacing.EAST; a=new net.minecraft.util.Vec3(d0,a.field_72448_b+d7*d3,a.field_72449_c+d8*d3); }
            else if(d4<d5) { face=j>i1?net.minecraft.util.EnumFacing.DOWN:net.minecraft.util.EnumFacing.UP; a=new net.minecraft.util.Vec3(a.field_72450_a+d6*d4,d1,a.field_72449_c+d8*d4); }
            else { face=k>j1?net.minecraft.util.EnumFacing.NORTH:net.minecraft.util.EnumFacing.SOUTH; a=new net.minecraft.util.Vec3(a.field_72450_a+d6*d5,a.field_72448_b+d7*d5,d2); }
            l=net.minecraft.util.MathHelper.func_76128_c(a.field_72450_a)-(face==net.minecraft.util.EnumFacing.EAST?1:0);
            i1=net.minecraft.util.MathHelper.func_76128_c(a.field_72448_b)-(face==net.minecraft.util.EnumFacing.UP?1:0);
            j1=net.minecraft.util.MathHelper.func_76128_c(a.field_72449_c)-(face==net.minecraft.util.EnumFacing.SOUTH?1:0);
            pos=new BlockPos(l,i1,j1);
            net.minecraft.block.state.IBlockState st1=(net.minecraft.block.state.IBlockState)blockState(pos);
            net.minecraft.block.Block block1=st1.func_177230_c();
            if(!ignoreNoBox||block1.func_180640_a(w,pos,st1)!=null) {
                if(block1.func_176209_a(st1,liquid)) {
                    net.minecraft.util.MovingObjectPosition hit=block1.func_180636_a(w,pos,a,b);
                    if(hit!=null) return hit;
                } else last=miss(a,face,pos);
            }
        }
        return returnLast?last:null;
    }

    /** new MovingObjectPosition(MISS, hit, side, pos); the nested enum is reached by name (javac's classpath lacks it). */
    @SuppressWarnings({"unchecked","rawtypes"})
    private static net.minecraft.util.MovingObjectPosition miss(net.minecraft.util.Vec3 v,net.minecraft.util.EnumFacing f,BlockPos p) {
        try {
            Class t=Class.forName("net.minecraft.util.MovingObjectPosition$MovingObjectType",true,Live.class.getClassLoader());
            return (net.minecraft.util.MovingObjectPosition)net.minecraft.util.MovingObjectPosition.class.getConstructor(t,net.minecraft.util.Vec3.class,net.minecraft.util.EnumFacing.class,BlockPos.class).newInstance(Enum.valueOf(t,"MISS"),v,f,p);
        } catch(ReflectiveOperationException e) { throw new RuntimeException(e); }
    }

    /** World.getChunkFromChunkCoords: a Chunk at (x, z) of `w`, marked loaded when the column is mirrored. */
    public static Object chunk(Object w,int x,int z) throws Exception {
        long key=(long)x<<32|(z&0xffffffffL);
        Object c=CHUNKS.get(key);
        if(c==null) {
            c=allocate("net.minecraft.world.chunk.Chunk");
            set(c,"field_76637_e",w); set(c,"field_76635_g",x); set(c,"field_76647_h",z);
            CHUNKS.put(key,c);
        }
        set(c,"field_76636_d",BIOMES.containsKey(key));
        return c;
    }

    /** WorldProvider.generateLightBrightnessTable: ambient 0 (overworld/end), 0.1 (nether). */
    public static float brightness(int level) {
        float amb=noSky?0.1F:0.0F, f=1.0F-level/15.0F;
        return (1.0F-f)/(f*3.0F+1.0F)*(1.0F-amb)+amb;
    }

    /** A chunk column's biome ids (z<<4|x, 255 unassigned); an empty array drops the unloaded column. */
    public static void biomes(int x,int z,byte[] ids) {
        long key=(long)x<<32|(z&0xffffffffL);
        if(ids.length==0) { BIOMES.remove(key); CHUNKS.remove(key); } else BIOMES.put(key,ids.clone());
    }

    /** World.getBlockState: Block.BLOCK_STATE_IDS of the mirrored `id<<4|meta`; air outside 0..255 or unloaded. */
    public static Object blockState(BlockPos pos) {
        int y=pos.func_177956_o();
        int raw=0;
        if(y>=0&&y<256) {
            short[] s=sectionData(pos.func_177958_n()>>4,y>>4,pos.func_177952_p()>>4);
            if(s!=null) raw=s[(y&15)<<8|(pos.func_177952_p()&15)<<4|(pos.func_177958_n()&15)]&0xffff;
        }
        Object state=net.minecraft.block.Block.field_176229_d.func_148745_a(raw);
        return state!=null?state:net.minecraft.init.Blocks.field_150350_a.func_176223_P();
    }

    /** World.getBiomeGenForCoords on the client: the chunk's id, plains when unloaded or unassigned. */
    public static Object biome(BlockPos pos) {
        int x=pos.func_177958_n(), z=pos.func_177952_p();
        byte[] ids=BIOMES.get((long)(x>>4)<<32|((z>>4)&0xffffffffL));
        int id=ids==null?255:ids[(z&15)<<4|(x&15)]&255;
        return net.minecraft.world.biome.BiomeGenBase.func_180276_a(id,net.minecraft.world.biome.BiomeGenBase.field_76772_c);
    }

    /** The mirrored 16^3 section at section coords (x,y,z): 4096 entries `id<<4|meta` indexed y<<8|z<<4|x, or null. */
    public static short[] sectionData(int x,int y,int z) { return SECTIONS.get(x+","+y+","+z); }

    private static void action(String kind, Object... values) {
        if(ACTIONS.size()>=1024) throw new IllegalStateException("mod action queue full");
        JsonObject o=new JsonObject(); o.addProperty("kind",kind);
        o.add("values",JSON.toJsonTree(values)); ACTIONS.add(o);
    }
    private static void packet(Packet<?> packet) throws Exception {
        Integer id=EnumConnectionState.PLAY.func_179246_a(EnumPacketDirection.SERVERBOUND,packet);
        if(id==null) throw new IllegalArgumentException("not a serverbound play packet");
        ByteBuf bytes=Unpooled.buffer();
        try { PacketBuffer buffer=new PacketBuffer(bytes); buffer.func_150787_b(id); packet.func_148840_b(buffer);
            if(bytes.readableBytes()>2097152) throw new IllegalArgumentException("mod packet too large");
            byte[] data=new byte[bytes.readableBytes()]; bytes.readBytes(data);
            JsonObject o=new JsonObject(); o.addProperty("kind","packet"); o.add("values",JSON.toJsonTree(data));
            if(ACTIONS.size()>=1024) throw new IllegalStateException("mod action queue full"); ACTIONS.add(o);
        } finally { bytes.release(); }
    }
    public static String changes() throws Exception {
        JsonObject out=new JsonObject(); out.addProperty("generation",generation);
        JsonObject current=playerState(), delta=new JsonObject();
        for(Map.Entry<String,JsonElement> value:current.entrySet()) if(!value.getValue().equals(baseline.get(value.getKey()))) delta.add(value.getKey(),value.getValue());
        baseline=current;
        if(player!=null) out.add("player",delta);
        JsonArray actions=new JsonArray(); JsonObject a; while((a=ACTIONS.poll())!=null) actions.add(a); out.add("actions",actions);
        return JSON.toJson(out);
    }

    private static JsonObject playerState() throws Exception {
        JsonObject p=new JsonObject();
        if(player!=null) {
            p.addProperty("x",player.field_70165_t); p.addProperty("y",player.field_70163_u); p.addProperty("z",player.field_70161_v);
            p.addProperty("yaw",player.field_70177_z); p.addProperty("pitch",player.field_70125_A);
            p.addProperty("motion_x",player.field_70159_w); p.addProperty("motion_y",player.field_70181_x); p.addProperty("motion_z",player.field_70179_y);
            p.addProperty("on_ground",player.field_70122_E); p.addProperty("no_clip",player.field_70145_X);
            p.addProperty("sprinting",(player.func_70096_w().func_75683_a(0)&8)!=0);
            p.addProperty("selected_slot",player.field_71071_by.field_70461_c);
            p.addProperty("strafe",player.field_71158_b.field_78902_a); p.addProperty("forward",player.field_71158_b.field_78900_b);
            p.addProperty("jump",player.field_71158_b.field_78901_c); p.addProperty("sneak",player.field_71158_b.field_78899_d);
            // Vanilla bindings held down, by real keys or a mod's KeyBinding.setKeyBindState (Sprint, auto-clickers).
            JsonArray held=new JsonArray(); for(net.minecraft.client.settings.KeyBinding k:VANILLA_KEYS) if((Boolean)get(k,"field_74513_e")) held.add(new JsonPrimitive((String)get(k,"field_74515_c")));
            p.add("held",held);
        }
        return p;
    }

    public static Object invoke(String key,Object self,Object[] args) throws Exception {
        String method=key.substring(key.lastIndexOf('.')+1);
        if(method.equals("func_147297_a")||method.equals("func_179290_a")) { packet((Packet<?>)args[0]); return null; }
        if(method.equals("func_78764_a")) { action("attack",get(args[1],"field_145783_c")); return null; }
        if(method.equals("func_71038_i")) { action("swing"); return null; }
        if(method.equals("func_71165_d")) { action("chat",args[0]); return null; }
        // addChatMessage / addChatComponentMessage: shown locally, never sent.
        if(method.equals("func_145747_a")||method.equals("func_146105_b")) { if(args[0]!=null) action("local_chat",Class.forName("net.minecraft.util.IChatComponent$Serializer",true,Live.class.getClassLoader()).getMethod("func_150696_a",IChatComponent.class).invoke(null,args[0])); return null; }
        if(method.equals("func_78757_d")) return null;
        if(method.equals("func_78750_j")) return 0;
        if(self instanceof Entity) {
            Entity e=(Entity)self;
            // Entity/EntityLivingBase/EntityPlayer(SP) methods are in Entities; the keys below are the rest.
            Object handled=Entities.invoke(key,self,args); if(handled!=Entities.UNHANDLED) return handled;
            if(method.equals("func_145782_y")) return get(e,"field_145783_c");
            if(method.equals("func_70096_w")) return get(e,"field_70180_af");
            if(method.equals("func_174813_aQ")||method.equals("func_70046_E")) return get(e,"field_70121_D");
            if(method.equals("func_70005_c_")) return e instanceof EntityPlayer ? ((GameProfile)get(e,"field_146106_i")).getName() : e.getClass().getSimpleName();
            if(method.equals("func_146103_bH")) return get(e,"field_146106_i");
            if(method.equals("func_110124_au")) return e instanceof EntityPlayer ? ((GameProfile)get(e,"field_146106_i")).getId() : new UUID(0,(Integer)get(e,"field_145783_c"));
            if(method.equals("func_110143_aJ")) return e.func_70096_w().func_111145_d(6);
            if(method.equals("func_110138_aP")) return 20f;
            if(method.equals("func_70606_j")) { e.func_70096_w().func_75692_b(6,((Number)args[0]).floatValue()); return null; }
            if(method.equals("func_70089_S")) return !e.field_70128_L;
            if(method.equals("func_70067_L")) return true;
            if(method.equals("func_70090_H")) return false;
            if(method.equals("func_70047_e")) return e instanceof EntityPlayer ? 1.62f : e.field_70131_O*.85f;
            int mask=method.equals("func_70093_af")?2:method.equals("func_70051_ag")?8:method.equals("func_82150_aj")?32:0;
            if(mask!=0) return (e.func_70096_w().func_75683_a(0)&mask)!=0;
            mask=method.equals("func_70095_a")?2:method.equals("func_70031_b")?8:method.equals("func_82142_c")?32:0;
            if(mask!=0) { byte flags=e.func_70096_w().func_75683_a(0); e.func_70096_w().func_75692_b(0,Byte.valueOf((byte)((Boolean)args[0]?flags|mask:flags&~mask))); return null; }
            if(method.equals("func_70107_b")||method.equals("func_70080_a")) { e.field_70165_t=((Number)args[0]).doubleValue(); e.field_70163_u=((Number)args[1]).doubleValue(); e.field_70161_v=((Number)args[2]).doubleValue(); if(args.length>=5) { e.field_70177_z=((Number)args[3]).floatValue(); e.field_70125_A=((Number)args[4]).floatValue(); } return null; }
            if(method.equals("func_174791_d")||method.equals("func_174824_e")) return new Vec3(e.field_70165_t,e.field_70163_u+(method.equals("func_174824_e")?1.62:0),e.field_70161_v);
            if(method.equals("func_70068_e")||method.equals("func_70032_d")||method.equals("func_70092_e")||method.equals("func_70011_f")) {
                double x,y,z;
                if(args.length==1) { Entity b=(Entity)args[0]; x=b.field_70165_t; y=b.field_70163_u; z=b.field_70161_v; }
                else { x=((Number)args[0]).doubleValue(); y=((Number)args[1]).doubleValue(); z=((Number)args[2]).doubleValue(); }
                double d=Math.pow(e.field_70165_t-x,2)+Math.pow(e.field_70163_u-y,2)+Math.pow(e.field_70161_v-z,2);
                if(method.equals("func_70032_d")) return (float)Math.sqrt(d);
                return method.equals("func_70011_f")?Math.sqrt(d):d;
            }
            if(method.equals("func_71045_bC")||method.equals("func_70694_bm")) { InventoryPlayer i=((EntityPlayer)e).field_71071_by; return i.field_70462_a[i.field_70461_c]; }
        }
        if(self instanceof InventoryPlayer) {
            InventoryPlayer i=(InventoryPlayer)self;
            if(method.equals("func_70448_g")) return i.field_70462_a[i.field_70461_c];
            if(method.equals("func_70302_i_")) return 40;
            int n=((Number)args[0]).intValue(); ItemStack[] array=n<36?i.field_70462_a:i.field_70460_b; int at=n<36?n:n-36;
            if(at<0||at>=array.length) return null;
            if(method.equals("func_70299_a")) { array[at]=(ItemStack)args[1]; return null; } return array[at];
        }
        if(method.equals("func_150899_d")) return ITEMS.get(((Number)args[0]).intValue());
        if(method.equals("func_150891_b")) return args[0]==null?0:ITEM_IDS.get(args[0]);
        if(self instanceof ItemStack) {
            if(method.equals("func_77973_b")) return get(self,"field_151002_e");
            if(method.equals("func_77960_j")||method.equals("func_77952_i")) return get(self,"field_77991_e");
            if(method.equals("func_77964_b")) { set(self,"field_77991_e",args[0]); return null; }
            if(method.equals("func_77976_d")) return 64;
            if(method.equals("func_77978_p")) return get(self,"field_77990_d");
            if(method.equals("func_77982_d")) { set(self,"field_77990_d",args[0]); return null; }
            if(method.equals("func_82833_r")) return "Item "+ITEM_IDS.get(get(self,"field_151002_e"));
            if(method.equals("func_77946_l")) { ItemStack copy=allocate(ItemStack.class); for(String f:new String[]{"field_151002_e","field_77991_e","field_77994_a","field_77990_d"}) set(copy,f,get(self,f)); return copy; }
        }
        if(method.equals("func_73045_a")) return ENTITIES.get(((Number)args[0]).intValue());
        // getPlayerEntityByUUID / getPlayerEntityByName: vanilla's linear scan of playerEntities.
        if(method.equals("func_152378_a")||method.equals("func_72924_a")) {
            boolean byId=method.equals("func_152378_a");
            for(Entity e:ENTITIES.values()) if(e instanceof EntityPlayer) {
                GameProfile g=(GameProfile)get(e,"field_146106_i");
                if(g!=null&&args[0]!=null&&args[0].equals(byId?g.getId():g.getName())) return e;
            }
            return null;
        }
        if(method.equals("func_82737_E")) return totalTime;
        if(method.equals("func_72820_D")) return time;
        if(method.equals("func_180495_p")) return blockState((BlockPos)args[0]);
        if(method.equals("func_175623_d")) return ((net.minecraft.block.state.IBlockState)blockState((BlockPos)args[0])).func_177230_c().func_149688_o()==net.minecraft.block.material.Material.field_151579_a;
        if(method.equals("func_180494_b")) return biome((BlockPos)args[0]);
        if(method.equals("func_175642_b")) return lightFor(args[0]==net.minecraft.world.EnumSkyBlock.SKY,(BlockPos)args[1]);
        if(method.equals("func_175705_a")) return lightFromNeighborsFor(args[0]==net.minecraft.world.EnumSkyBlock.SKY,(BlockPos)args[1]);
        if(method.equals("func_175626_b")) return lightFromNeighborsFor(true,(BlockPos)args[0])<<20|Math.max(lightFromNeighborsFor(false,(BlockPos)args[0]),(Integer)args[1])<<4;
        if(method.equals("func_175699_k")) { BlockPos p=(BlockPos)args[0]; if(p.func_177956_o()<0) return 0; if(p.func_177956_o()>=256) p=new BlockPos(p.func_177958_n(),255,p.func_177952_p()); return lightSubtracted(p,0); }
        if(method.equals("func_175721_c")) return light((BlockPos)args[0],(Boolean)args[1]);
        if(method.equals("func_175671_l")) return light((BlockPos)args[0],true);
        if(method.equals("func_175724_o")) return brightness(light((BlockPos)args[0],true));
        if(method.equals("func_175657_ab")) return skylightSubtracted;
        if(method.equals("func_175678_i")) { BlockPos p=(BlockPos)args[0]; return p.func_177956_o()>=256||openAbove(p)&&lightFor(true,p)>=15; }
        return null;
    }
}
