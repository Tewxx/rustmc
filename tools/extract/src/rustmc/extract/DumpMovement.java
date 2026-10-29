package rustmc.extract;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.google.gson.JsonPrimitive;
import com.mojang.authlib.GameProfile;
import java.lang.reflect.Field;
import java.util.*;
import net.minecraft.block.Block;
import net.minecraft.block.properties.IProperty;
import net.minecraft.block.state.IBlockState;
import net.minecraft.client.Minecraft;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.client.multiplayer.PlayerControllerMP;
import net.minecraft.client.network.NetHandlerPlayClient;
import net.minecraft.client.network.NetworkPlayerInfo;
import net.minecraft.client.settings.GameSettings;
import net.minecraft.client.settings.KeyBinding;
import net.minecraft.entity.Entity;
import net.minecraft.init.Blocks;
import net.minecraft.init.Items;
import net.minecraft.item.ItemStack;
import net.minecraft.profiler.Profiler;
import net.minecraft.util.AxisAlignedBB;
import net.minecraft.util.BlockPos;
import net.minecraft.util.MovementInputFromOptions;
import net.minecraft.world.World;
import net.minecraft.world.WorldProviderSurface;
import net.minecraft.world.WorldSettings;
import net.minecraft.world.WorldType;
import net.minecraft.world.chunk.IChunkProvider;
import net.minecraft.world.storage.SaveHandlerMP;
import net.minecraft.world.storage.WorldInfo;

/**
 * Runs the real `EntityPlayerSP.onUpdate` (Entity / EntityLivingBase / EntityPlayer / EntityPlayerSP
 * tick path, `MovementInputFromOptions`) against a block map with no chunks, for scripted inputs.
 * Only the packet send (`onUpdateWalkingPlayer`, `sendPlayerAbilities`) is stubbed, since there is no
 * connection. Output: golden/movement.json (doubles and floats as raw bits).
 */
final class DumpMovement {
    static final int F = 1, B = 2, L = 4, R = 8, J = 16, S = 32, P = 64, U = 128;

    /** The block map as a `World`; every chunk is loaded, no entities exist. */
    static final class MoveWorld extends World {
        final Map<BlockPos, IBlockState> states = new HashMap<BlockPos, IBlockState>();
        MoveWorld() {
            super(new SaveHandlerMP(), new WorldInfo(new WorldSettings(0L, WorldSettings.GameType.SURVIVAL,
                false, false, WorldType.DEFAULT), "movement"), new WorldProviderSurface(), new Profiler(), true);
        }
        protected IChunkProvider createChunkProvider() { return null; }
        protected int getRenderDistanceChunks() { return 8; }
        protected boolean isChunkLoaded(int x, int z, boolean allowEmpty) { return true; }
        public IBlockState getBlockState(BlockPos pos) {
            if (pos.getX() < -30000000 || pos.getZ() < -30000000 || pos.getX() >= 30000000 || pos.getZ() >= 30000000 || pos.getY() < 0 || pos.getY() >= 256) return Blocks.air.getDefaultState();
            IBlockState s = states.get(pos);
            return s != null ? s : Blocks.air.getDefaultState();
        }
        net.minecraft.world.chunk.Chunk chunk;
        public net.minecraft.world.chunk.Chunk getChunkFromChunkCoords(int x, int z) {
            if (chunk == null) {
                chunk = new net.minecraft.world.chunk.Chunk(this, 0, 0);
                chunk.setChunkLoaded(true);
            }
            return chunk;
        }
        public List<Entity> getEntitiesWithinAABBExcludingEntity(Entity e, AxisAlignedBB bb) { return new ArrayList<Entity>(); }
    }

    /** Real `EntityPlayerSP`; there is no server, so the position/ability packets are dropped. */
    static final class TestPlayer extends EntityPlayerSP {
        TestPlayer(Minecraft mc, World w, NetHandlerPlayClient nh) { super(mc, w, nh, null); }
        public void onUpdateWalkingPlayer() {}
        public void sendPlayerAbilities() {}
    }

    static final class Tick {
        int keys; float yaw = Float.NaN; double[] motion;
    }

    static final class Sc {
        final String name;
        final List<Object[]> fills = new ArrayList<Object[]>();
        final List<Tick> ticks = new ArrayList<Tick>();
        double x = 0.5, y = 64, z = 0.5;
        float yaw = 0;
        String mode = "survival";
        boolean flying;
        boolean armSword;
        double[] motion0;
        final List<int[]> fx = new ArrayList<int[]>();
        Sc(String name) { this.name = name; }
        Sc floor() { return fill(-24, 60, -24, 24, 63, 24, "stone"); }
        Sc fill(int x1, int y1, int z1, int x2, int y2, int z2, String s) { fills.add(new Object[] {x1, y1, z1, x2, y2, z2, s}); return this; }
        Sc set(int x, int y, int z, String s) { return fill(x, y, z, x, y, z, s); }
        Sc rep(int n, int keys) { return rep(n, keys, Float.NaN); }
        Sc rep(int n, int keys, float yaw) { for (int i = 0; i < n; i++) { Tick t = new Tick(); t.keys = keys; t.yaw = yaw; ticks.add(t); } return this; }
        /** Repeats a key set with yaw = start + i * step. */
        Sc sweep(int n, int keys, float start, float step) {
            for (int i = 0; i < n; i++) { Tick t = new Tick(); t.keys = keys; t.yaw = start + i * step; ticks.add(t); }
            return this;
        }
        /** The next tick starts with this velocity (knockback, explosion, piston...). */
        Sc knock(double mx, double my, double mz, int keys) {
            Tick t = new Tick(); t.keys = keys; t.motion = new double[] {mx, my, mz}; ticks.add(t); return this;
        }
    }

    static final Map<IBlockState, Integer> INDEX = new HashMap<IBlockState, Integer>();

    /** `name[prop=value,...]` over the block registry. */
    static IBlockState st(String spec) {
        String name = spec, props = "";
        int i = spec.indexOf('[');
        if (i >= 0) { name = spec.substring(0, i); props = spec.substring(i + 1, spec.length() - 1); }
        Block b = Block.getBlockFromName(name);
        if (b == null) throw new IllegalArgumentException("no block " + name);
        IBlockState s = b.getDefaultState();
        if (props.length() > 0) {
            for (String kv : props.split(",")) {
                String[] p = kv.split("=");
                boolean done = false;
                for (Object o : s.getPropertyNames()) {
                    IProperty prop = (IProperty) o;
                    if (!prop.getName().equals(p[0])) continue;
                    for (Object v : prop.getAllowedValues()) {
                        if (prop.getName((Comparable) v).equals(p[1])) { s = s.withProperty(prop, (Comparable) v); done = true; break; }
                    }
                }
                if (!done) throw new IllegalArgumentException("bad property " + kv + " on " + name);
            }
        }
        return s;
    }

    static sun.misc.Unsafe unsafe;
    static Object alloc(Class<?> c) throws Exception { return unsafe.allocateInstance(c); }
    static void put(Object o, Class<?> c, String field, Object v) throws Exception {
        Field f = c.getDeclaredField(field); f.setAccessible(true); f.set(o, v);
    }

    static JsonArray dump() throws Exception {
        Field uf = sun.misc.Unsafe.class.getDeclaredField("theUnsafe"); uf.setAccessible(true);
        unsafe = (sun.misc.Unsafe) uf.get(null);
        int n = 0;
        for (Block b : Block.blockRegistry) for (IBlockState s : b.getBlockState().getValidStates()) INDEX.put(s, n++);
        JsonArray out = new JsonArray();
        for (Sc sc : scenarios()) out.add(run(sc));
        return out;
    }

    static JsonObject run(Sc sc) throws Exception {
        MoveWorld world = new MoveWorld();
        JsonArray blocks = new JsonArray();
        for (Object[] f : sc.fills) {
            IBlockState s = st((String) f[6]);
            int x1 = (Integer) f[0], y1 = (Integer) f[1], z1 = (Integer) f[2], x2 = (Integer) f[3], y2 = (Integer) f[4], z2 = (Integer) f[5];
            for (int x = x1; x <= x2; x++) for (int y = y1; y <= y2; y++) for (int z = z1; z <= z2; z++) world.states.put(new BlockPos(x, y, z), s);
            JsonArray a = new JsonArray();
            for (int v : new int[] {x1, y1, z1, x2, y2, z2, INDEX.get(s)}) a.add(Json.num(v));
            blocks.add(a);
        }
        WorldSettings.GameType gt = sc.mode.equals("creative") ? WorldSettings.GameType.CREATIVE
            : sc.mode.equals("spectator") ? WorldSettings.GameType.SPECTATOR : WorldSettings.GameType.SURVIVAL;

        Minecraft mc = (Minecraft) alloc(Minecraft.class);
        GameSettings gs = new GameSettings();
        put(mc, Minecraft.class, "gameSettings", gs);
        PlayerControllerMP pc = (PlayerControllerMP) alloc(PlayerControllerMP.class);
        put(pc, PlayerControllerMP.class, "currentGameType", gt);
        put(mc, Minecraft.class, "playerController", pc);
        put(mc, Minecraft.class, "theMinecraft", mc);
        NetHandlerPlayClient nh = (NetHandlerPlayClient) alloc(NetHandlerPlayClient.class);
        UUID id = UUID.fromString("00000000-0000-0000-0000-000000000001");
        put(nh, NetHandlerPlayClient.class, "profile", new GameProfile(id, "movement"));
        Map<UUID, NetworkPlayerInfo> infos = new HashMap<UUID, NetworkPlayerInfo>();
        put(nh, NetHandlerPlayClient.class, "playerInfoMap", infos);
        if (gt == WorldSettings.GameType.SPECTATOR) {
            NetworkPlayerInfo info = (NetworkPlayerInfo) alloc(NetworkPlayerInfo.class);
            put(info, NetworkPlayerInfo.class, "gameType", gt);
            infos.put(id, info);
        }

        TestPlayer p = new TestPlayer(mc, world, nh);
        put(mc, Minecraft.class, "thePlayer", p);
        put(mc, Minecraft.class, "renderViewEntity", p);
        p.movementInput = new MovementInputFromOptions(gs);
        gt.configurePlayerCapabilities(p.capabilities);
        if (sc.flying) p.capabilities.isFlying = true;
        p.setPosition(sc.x, sc.y, sc.z);
        p.rotationYaw = sc.yaw; p.prevRotationYaw = sc.yaw; p.rotationYawHead = sc.yaw; p.prevRotationYawHead = sc.yaw;
        if (sc.motion0 != null) { p.motionX = sc.motion0[0]; p.motionY = sc.motion0[1]; p.motionZ = sc.motion0[2]; }
        p.onGround = false;
        for (int[] e : sc.fx) p.addPotionEffect(new net.minecraft.potion.PotionEffect(e[0], 100000, e[1]));

        JsonArray script = new JsonArray(), rows = new JsonArray();
        KeyBinding[] kb = {gs.keyBindForward, gs.keyBindBack, gs.keyBindLeft, gs.keyBindRight, gs.keyBindJump, gs.keyBindSneak, gs.keyBindSprint};
        Field pressed = KeyBinding.class.getDeclaredField("pressed"); pressed.setAccessible(true);
        ItemStack sword = new ItemStack(Items.iron_sword);
        if (sc.armSword) p.inventory.setInventorySlotContents(0, sword);
        for (Tick t : sc.ticks) {
            for (int i = 0; i < 7; i++) pressed.setBoolean(kb[i], (t.keys & (1 << i)) != 0);
            // `using_item`: the held sword is being used (blocking), as `isUsingItem()` sees it.
            Field iu = EntityPlayerSPItem.itemInUse, iuc = EntityPlayerSPItem.itemInUseCount;
            if ((t.keys & U) != 0) { iu.set(p, sword); iuc.setInt(p, 100000); } else { iu.set(p, null); iuc.setInt(p, 0); }
            if (!Float.isNaN(t.yaw)) p.rotationYaw = t.yaw;
            if (t.motion != null) { p.motionX = t.motion[0]; p.motionY = t.motion[1]; p.motionZ = t.motion[2]; }
            JsonArray tk = new JsonArray();
            tk.add(Json.num(t.keys));
            tk.add(Float.isNaN(t.yaw) ? new JsonPrimitive(-1L << 40) : new JsonPrimitive(Float.floatToRawIntBits(t.yaw)));
            if (t.motion != null) for (double d : t.motion) tk.add(new JsonPrimitive(Double.doubleToRawLongBits(d)));
            script.add(tk);

            p.onUpdate();

            JsonArray r = new JsonArray();
            for (double d : new double[] {p.posX, p.posY, p.posZ, p.motionX, p.motionY, p.motionZ}) r.add(new JsonPrimitive(Double.doubleToRawLongBits(d)));
            int fl = (p.onGround ? 1 : 0) | (p.isCollidedHorizontally ? 2 : 0) | (p.isCollidedVertically ? 4 : 0)
                | (p.isSprinting() ? 8 : 0) | (p.capabilities.isFlying ? 16 : 0) | (p.isInWater() ? 32 : 0);
            r.add(Json.num(fl));
            r.add(new JsonPrimitive(Float.floatToRawIntBits(p.fallDistance)));
            rows.add(r);
        }
        JsonObject o = new JsonObject();
        o.addProperty("name", sc.name);
        o.addProperty("mode", sc.mode);
        o.addProperty("flying", sc.flying);
        o.add("start", bits(sc.x, sc.y, sc.z));
        o.addProperty("yaw", Float.floatToRawIntBits(sc.yaw));
        o.add("motion0", sc.motion0 == null ? new JsonArray() : bits(sc.motion0[0], sc.motion0[1], sc.motion0[2]));
        JsonArray fxa = new JsonArray();
        for (int[] e : sc.fx) { JsonArray a = new JsonArray(); a.add(Json.num(e[0])); a.add(Json.num(e[1])); fxa.add(a); }
        o.add("fx", fxa);
        o.add("blocks", blocks);
        o.add("script", script);
        o.add("out", rows);
        return o;
    }

    static final class EntityPlayerSPItem {
        static Field itemInUse, itemInUseCount;
        static {
            try {
                itemInUse = net.minecraft.entity.player.EntityPlayer.class.getDeclaredField("itemInUse"); itemInUse.setAccessible(true);
                itemInUseCount = net.minecraft.entity.player.EntityPlayer.class.getDeclaredField("itemInUseCount"); itemInUseCount.setAccessible(true);
            } catch (Exception e) { throw new RuntimeException(e); }
        }
    }

    static JsonArray bits(double a, double b, double c) {
        JsonArray r = new JsonArray();
        r.add(new JsonPrimitive(Double.doubleToRawLongBits(a))); r.add(new JsonPrimitive(Double.doubleToRawLongBits(b))); r.add(new JsonPrimitive(Double.doubleToRawLongBits(c)));
        return r;
    }

    // ---- scenarios -------------------------------------------------------------------------

    static List<Sc> scenarios() {
        List<Sc> l = new ArrayList<Sc>();
        Sc s;

        s = new Sc("flat_walk").floor();
        s.rep(10, 0).rep(20, F).rep(30, F | P).rep(20, 0).rep(10, B).rep(10, L).rep(10, R).rep(10, F | L).rep(10, F | R).rep(10, B | L).rep(10, B | R).rep(15, 0);
        l.add(s);

        s = new Sc("flat_yaw_sweep").floor();
        s.sweep(60, F | L, 0, 7.3f).sweep(60, F | P, 90, -11.9f).sweep(40, B | R, -170, 8.1f).rep(20, 0);
        l.add(s);

        s = new Sc("sprint_double_tap").floor();
        s.rep(3, F).rep(2, 0).rep(25, F).rep(10, 0).rep(3, F).rep(1, 0).rep(2, F).rep(15, F | S).rep(10, F).rep(5, 0).rep(2, F | B).rep(10, F | L | P);
        l.add(s);

        s = new Sc("jump_sprint_repeat").floor();
        s.rep(60, F | P | J).rep(30, J).rep(20, 0).rep(30, F | J | L, 33f).rep(20, B | J | P);
        l.add(s);

        s = new Sc("wall_diag").floor().fill(-24, 64, 6, 24, 70, 6, "stone");
        s.rep(40, F | P, 20f).rep(10, F | P, 160f).rep(30, F | L | P, 0f).rep(20, J | F | P, 5f);
        l.add(s);

        s = new Sc("corner_slide").floor().fill(3, 64, -24, 3, 70, 24, "stone").fill(-24, 64, 8, 24, 70, 8, "stone");
        s.sweep(80, F | P, -80, 0.2f).rep(30, F | L | P, 135f).rep(30, B | R, 45f);
        l.add(s);

        s = new Sc("ceiling_jump").floor().fill(-24, 66, -24, 24, 66, 24, "stone");
        s.rep(60, J).rep(20, F | P | J, 10f);
        l.add(s);

        s = new Sc("ceiling_low").floor().fill(-24, 67, -24, 24, 67, 24, "stone").fill(-24, 66, 6, 24, 66, 24, "stone");
        s.rep(40, J).rep(40, J | F | P);
        l.add(s);

        s = new Sc("step_slab").floor();
        s.fill(-24, 64, 6, 24, 64, 6, "stone_slab[half=bottom]").fill(-24, 64, 10, 24, 64, 10, "stone_slab[half=top]").fill(-24, 65, 10, 24, 65, 10, "air");
        s.rep(30, F | P).rep(30, F | P, 0f).rep(20, F | L | P, 0f);
        l.add(s);

        s = new Sc("step_stairs");
        s.floor().fill(0, 64, 5, 0, 64, 5, "oak_stairs[facing=south,half=bottom]");
        for (int i = 0; i < 5; i++) {
            s.fill(-3, 64 + i, 5 + i, 3, 64 + i, 5 + i, "oak_stairs[facing=north,half=bottom,shape=straight]");
            s.fill(-3, 64, 5 + i, 3, 64 + i - 1, 5 + i, "stone");
        }
        s.rep(60, F | P).rep(20, F | L | P).rep(30, B | P);
        l.add(s);

        s = new Sc("step_stairs_shapes").floor();
        s.fill(-3, 64, 5, -3, 64, 5, "oak_stairs[facing=east,half=bottom,shape=inner_left]");
        s.fill(0, 64, 5, 0, 64, 5, "oak_stairs[facing=south,half=bottom,shape=outer_right]");
        s.fill(3, 64, 5, 3, 64, 5, "oak_stairs[facing=west,half=top,shape=straight]");
        s.fill(6, 64, 5, 6, 64, 5, "stone_stairs[facing=north,half=top,shape=straight]");
        s.set(-3, 64, 6, "stone");
        s.x = -3.5; s.z = 0.5;
        s.rep(20, F | P).rep(10, J).rep(20, 0).rep(30, F | P | L, -45f).rep(30, F | R, 15f).rep(30, F | P | R, 80f);
        l.add(s);

        s = new Sc("full_block_jump").floor().fill(-24, 64, 6, 24, 64, 6, "stone").fill(-24, 65, 12, 24, 65, 12, "stone").fill(-24, 64, 12, 24, 64, 12, "stone");
        s.rep(20, F | P).rep(40, F | P | J).rep(20, F | P | J, 0f);
        l.add(s);

        s = new Sc("fence_gate_wall").floor();
        s.fill(-24, 64, 5, 24, 64, 5, "fence").fill(-24, 64, 9, 24, 64, 9, "cobblestone_wall").fill(-24, 64, 13, 24, 64, 13, "fence_gate[facing=south,open=false]");
        s.fill(-24, 64, 17, 24, 64, 17, "fence_gate[facing=south,open=true]").fill(-24, 64, 21, 24, 64, 21, "fence");
        s.rep(20, F | P).rep(30, F | P | J).rep(30, F | P | J).rep(10, 0).rep(40, F | P | J, 0f);
        l.add(s);

        s = new Sc("trapdoor_carpet_snow").floor();
        s.fill(-24, 64, 5, 24, 64, 5, "carpet").fill(-24, 64, 8, 24, 64, 8, "snow_layer[layers=3]").fill(-24, 64, 11, 24, 64, 11, "trapdoor[facing=north,half=bottom,open=false]");
        s.fill(-24, 64, 14, 24, 64, 14, "trapdoor[facing=north,half=top,open=true]").fill(-24, 64, 17, 24, 64, 17, "snow_layer[layers=8]").fill(-24, 64, 20, 24, 64, 20, "cake[bites=2]");
        s.rep(120, F | P);
        l.add(s);

        s = new Sc("sneak_ledge").floor().fill(-24, 55, 6, 24, 63, 24, "air");
        s.rep(40, F | S).rep(20, S).rep(30, F | L | S, 0f).rep(30, F | R | S, 30f).rep(30, B | S, 180f).rep(15, F | S | J, 0f).rep(30, F, 0f).rep(40, 0);
        l.add(s);

        s = new Sc("sneak_corner_ledge").floor().fill(-24, 55, 6, 24, 63, 24, "air").fill(-24, 55, -24, -2, 63, 24, "air");
        s.x = 0.9; s.z = 0.5;
        s.rep(30, S | F | L, 0f).rep(30, S | L, 0f).rep(30, S | F | R, 0f).rep(30, S | B | L, 0f);
        l.add(s);

        s = new Sc("ladder_up");
        s.floor().fill(-2, 64, 4, 2, 72, 4, "stone").fill(0, 64, 3, 0, 72, 3, "ladder[facing=north]");
        s.z = 2.5;
        s.rep(30, F).rep(15, F | S).rep(10, 0).rep(15, F | S).rep(10, F | P).rep(10, B, 0f).rep(30, 0);
        l.add(s);

        s = new Sc("ladder_fall_catch");
        s.floor().fill(-2, 64, 4, 2, 80, 4, "stone").fill(0, 64, 3, 0, 80, 3, "ladder[facing=north]").fill(2, 70, 3, 2, 70, 3, "air");
        s.x = 0.5; s.y = 76; s.z = 2.5; s.motion0 = new double[] {0, -0.3, 0.1};
        s.rep(10, 0).rep(20, F).rep(30, S).rep(10, B | J).rep(30, 0);
        l.add(s);

        s = new Sc("vine_climb");
        s.floor().fill(-2, 64, 4, 2, 72, 4, "stone").fill(0, 64, 3, 0, 72, 3, "vine[south=true]").fill(2, 64, 3, 2, 72, 3, "vine[south=true,east=false,north=false,west=false,up=false]");
        s.z = 2.5;
        s.rep(30, F).rep(20, 0).rep(15, F | S).rep(30, J | F).rep(40, 0);
        l.add(s);

        s = new Sc("water_pool").floor();
        s.fill(-6, 62, 4, 6, 63, 16, "water").fill(-6, 61, 4, 6, 61, 16, "stone").fill(-6, 62, 4, 6, 63, 4, "stone");
        s.rep(10, F | P).rep(30, F | P | J, 0f).rep(30, F).rep(20, S).rep(20, J).rep(40, F | L | J, 45f).rep(20, 0);
        l.add(s);

        s = new Sc("water_dive_fall");
        s.floor().fill(-6, 61, 0, 6, 63, 8, "water").fill(-6, 60, 0, 6, 60, 8, "stone");
        s.y = 70;
        s.rep(30, 0).rep(30, S).rep(30, J).rep(20, F);
        l.add(s);

        s = new Sc("water_exit_ledge").floor();
        s.fill(-6, 62, 0, 6, 63, 6, "water").fill(-6, 62, 7, 6, 64, 8, "stone").fill(-6, 64, 7, 6, 64, 8, "stone");
        s.fill(-6, 62, 0, 6, 62, 6, "water").set(0, 64, 7, "stone");
        s.y = 62; s.z = 3.5;
        s.rep(30, F).rep(30, F | J).rep(30, F | J | P, 0f).rep(20, 0);
        l.add(s);

        s = new Sc("water_flowing").floor().fill(-6, 62, 4, 6, 63, 12, "stone");
        for (int i = 0; i < 8; i++) s.fill(-6, 64, 4 + i, 6, 64, 4 + i, "flowing_water[level=" + i + "]");
        s.fill(-6, 64, 12, 6, 64, 12, "flowing_water[level=8]");
        s.rep(10, 0).rep(40, F, 0f).rep(30, J).rep(40, 0).rep(20, F | L | S);
        l.add(s);

        s = new Sc("water_waterfall");
        s.floor().fill(-3, 64, 4, 3, 70, 4, "stone").fill(0, 64, 3, 0, 70, 3, "flowing_water[level=8]").fill(0, 71, 3, 0, 71, 3, "water");
        s.fill(0, 63, 3, 0, 63, 3, "water");
        s.z = 1.5;
        s.rep(10, 0).rep(20, F | P, 0f).rep(30, F).rep(30, 0);
        l.add(s);

        s = new Sc("lava_pool").floor();
        s.fill(-6, 62, 4, 6, 63, 16, "lava").fill(-6, 61, 4, 6, 61, 16, "stone");
        s.rep(10, F | P).rep(30, F | P | J, 0f).rep(30, F).rep(20, S).rep(30, J).rep(20, 0);
        l.add(s);

        s = new Sc("lava_exit_flowing").floor();
        s.fill(-6, 62, 0, 6, 63, 6, "lava").fill(-6, 61, 0, 6, 61, 6, "stone").fill(-6, 62, 7, 6, 64, 8, "stone");
        s.fill(-6, 64, 0, 6, 64, 6, "flowing_lava[level=2]");
        s.y = 62; s.z = 3.5;
        s.rep(40, F | J).rep(20, J | F | P).rep(20, 0);
        l.add(s);

        s = new Sc("soul_sand");
        s.floor().fill(-24, 63, -24, 24, 63, 24, "soul_sand");
        s.rep(40, F | P).rep(20, J | F).rep(30, F | L, 40f).rep(20, 0);
        l.add(s);

        s = new Sc("ice_slide");
        s.floor().fill(-24, 63, -24, 24, 63, 24, "ice").fill(-24, 63, 8, 24, 63, 24, "packed_ice");
        s.rep(30, F | P).rep(30, 0).rep(20, B).rep(20, L, 90f).rep(30, F | J | P, 0f).rep(40, 0);
        l.add(s);

        s = new Sc("slime_bounce");
        s.floor().fill(-24, 63, -24, 24, 63, 24, "slime");
        s.y = 70;
        s.rep(120, 0).rep(60, S).rep(60, F | P | J);
        l.add(s);

        s = new Sc("slime_walk");
        s.floor().fill(-4, 63, 0, 4, 63, 16, "slime");
        s.rep(30, F | P).rep(30, F | P | J).rep(30, 0).rep(30, F | S);
        l.add(s);

        s = new Sc("cobweb");
        s.floor().fill(-24, 64, 5, 24, 65, 7, "web");
        s.rep(30, F | P).rep(20, J).rep(40, F | J).rep(20, 0);
        l.add(s);

        s = new Sc("cobweb_fall");
        s.floor().fill(-3, 66, -3, 3, 66, 3, "web");
        s.y = 80;
        s.rep(80, 0).rep(40, F);
        l.add(s);

        s = new Sc("fall_fall_distance").floor();
        s.y = 90;
        s.rep(120, 0).rep(10, F | P).rep(30, J);
        l.add(s);

        s = new Sc("knockback").floor().fill(-24, 64, 8, 24, 70, 8, "stone");
        s.rep(5, 0).knock(0.4, 0.36, 0.0, 0).rep(40, 0).knock(0.0, 0.36, 0.4, F | P).rep(30, F | P).knock(-0.4, 0.2, -0.4, F | P).rep(30, F | P | L, 20f);
        s.knock(0.3, 0.6, 0.3, J).rep(20, J | F).knock(0.0, 0.8, 0.0, S).rep(20, S);
        l.add(s);

        s = new Sc("knockback_air_ceiling").floor().fill(-24, 67, -24, 24, 67, 24, "stone");
        s.rep(3, 0).knock(0.2, 1.2, 0.2, 0).rep(30, 0).knock(1.2, 0.0, 0.0, F).rep(30, F);
        l.add(s);

        s = new Sc("creative_flight"); s.mode = "creative";
        s.floor();
        s.rep(3, 0).rep(1, J).rep(1, 0).rep(1, J).rep(15, J).rep(20, F | P).rep(10, F).rep(15, S).rep(10, F | S).rep(20, J | F | P, 33f).rep(15, S).rep(20, 0).rep(40, S).rep(20, F | P);
        l.add(s);

        s = new Sc("creative_walk_jump"); s.mode = "creative";
        s.floor().fill(-24, 64, 8, 24, 70, 8, "stone");
        s.rep(30, F | P).rep(10, J).rep(40, F | P | J).rep(10, 0).rep(4, J | F | P);
        l.add(s);

        s = new Sc("creative_fly_wall_ceiling"); s.mode = "creative"; s.flying = true; s.y = 66;
        s.floor().fill(-24, 64, 8, 24, 80, 8, "stone").fill(-24, 72, -24, 24, 72, 24, "stone");
        s.rep(30, F | P).rep(20, J | F | P).rep(20, J).rep(20, S | B).rep(10, 0);
        l.add(s);

        s = new Sc("spectator_noclip"); s.mode = "spectator"; s.flying = true;
        s.floor().fill(-24, 64, 5, 24, 70, 8, "stone").fill(-24, 71, 0, 24, 71, 24, "stone");
        s.rep(30, F | P).rep(20, F).rep(20, J).rep(20, S | B).rep(20, F | P, 0f);
        l.add(s);

        s = new Sc("using_item_slow"); s.armSword = true;
        s.floor();
        s.rep(20, F | U).rep(20, F | P | U).rep(10, F | P).rep(20, F | P | U | J).rep(10, F | P | S | U).rep(10, F | P);
        l.add(s);

        s = new Sc("stuck_in_block").floor().fill(1, 64, 0, 1, 65, 2, "stone");
        s.x = 0.8; s.z = 0.5;
        s.rep(10, 0).rep(10, F);
        l.add(s);

        s = new Sc("stuck_in_block_corner").floor().fill(1, 64, 1, 1, 65, 1, "stone").fill(0, 64, 1, 0, 65, 1, "stone");
        s.x = 0.9; s.z = 0.9;
        s.rep(10, 0).rep(10, F | R);
        l.add(s);

        s = new Sc("stuck_in_floor").floor().fill(0, 64, 0, 0, 65, 0, "stone");
        s.x = 0.5; s.z = 0.5;
        s.rep(10, 0).rep(10, F);
        l.add(s);

        s = new Sc("negative_coords").floor().fill(-10, 64, -14, -10, 66, -14, "stone").fill(-8, 64, -20, -4, 64, -20, "stone_slab[half=bottom]");
        s.x = -10.3; s.z = -7.7; s.yaw = 180f;
        s.rep(40, F | P).sweep(60, F | L | P, 180, 3f).rep(30, B | R, 90f);
        l.add(s);

        s = new Sc("bed_and_chest").floor();
        s.fill(-24, 64, 5, 24, 64, 5, "chest[facing=north]").fill(-24, 64, 9, 24, 64, 9, "anvil[facing=north]").fill(-24, 64, 13, 24, 64, 13, "cauldron[level=0]");
        s.fill(-24, 64, 17, 24, 64, 17, "enchanting_table").fill(-24, 64, 21, 24, 64, 21, "flower_pot");
        s.rep(120, F | P | J);
        l.add(s);

        s = new Sc("lily_pad_cactus_misc").floor();
        s.fill(-24, 64, 5, 24, 64, 5, "cactus").fill(-24, 64, 8, 24, 64, 8, "waterlily").fill(-24, 64, 11, 24, 64, 11, "iron_bars");
        s.fill(-24, 64, 14, 24, 64, 14, "glass_pane").fill(-24, 64, 17, 24, 64, 17, "end_portal_frame[eye=false,facing=north]").fill(-24, 64, 20, 24, 64, 20, "brewing_stand");
        s.rep(120, F | P | J);
        l.add(s);

        s = new Sc("effects_jump_boost").floor(); s.fx.add(new int[] {8, 1});
        s.rep(40, F | P | J).rep(40, J).rep(20, 0);
        l.add(s);

        s = new Sc("effects_blindness_sprint").floor(); s.fx.add(new int[] {15, 0});
        s.rep(40, F | P).rep(20, F | P | J);
        l.add(s);

        s = new Sc("effects_jump_boost_ceiling").floor().fill(-24, 67, -24, 24, 67, 24, "stone"); s.fx.add(new int[] {8, 3});
        s.rep(60, J | F | P);
        l.add(s);

        for (int seed = 1; seed <= 10; seed++) l.add(fuzz(seed));
        return l;
    }

    static final String[] PALETTE = {
        "stone", "stone_slab[half=bottom]", "stone_slab[half=top]", "oak_stairs[facing=north,half=bottom]", "oak_stairs[facing=east,half=bottom]",
        "oak_stairs[facing=south,half=top]", "oak_stairs[facing=west,half=bottom]", "fence", "cobblestone_wall", "fence_gate[facing=north,open=false]",
        "ladder[facing=north]", "ladder[facing=east]", "vine[south=true]", "web", "soul_sand", "ice", "slime", "carpet", "snow_layer[layers=2]",
        "snow_layer[layers=5]", "trapdoor[facing=north,half=bottom,open=false]", "trapdoor[facing=south,half=top,open=true]", "cactus",
        "water", "flowing_water[level=3]", "flowing_water[level=8]", "lava", "chest[facing=north]", "dirt", "iron_bars", "glass_pane", "cake[bites=1]",
        "packed_ice", "stone_stairs[facing=south,half=bottom]", "stone_stairs[facing=east,half=top]", "stone_slab[half=bottom]"
    };

    /** Random terrain and random inputs, seeded so the dump is reproducible. */
    static Sc fuzz(int seed) {
        Random r = new Random(7000 + seed);
        Sc s = new Sc("fuzz_" + seed).floor();
        s.mode = seed % 5 == 0 ? "creative" : "survival";
        int count = 40 + r.nextInt(80);
        for (int i = 0; i < count; i++) {
            int x = r.nextInt(17) - 8, z = r.nextInt(17) - 8, y = 64 + (r.nextInt(5) == 0 ? 1 : 0);
            String b = PALETTE[r.nextInt(PALETTE.length)];
            if (b.equals("water") || b.startsWith("flowing_water") || b.equals("lava")) { y = 63; }
            s.set(x, y, z, b);
            if (r.nextInt(6) == 0) s.set(x, 64 + 1, z, "stone");
        }
        if (r.nextBoolean()) s.fill(-8, 63, -8, 8, 63, 8, PALETTE[14 + r.nextInt(4)]);
        s.x = r.nextInt(5) - 2 + r.nextDouble();
        s.z = r.nextInt(5) - 2 + r.nextDouble();
        s.y = 64 + r.nextInt(3);
        float yaw = r.nextInt(360);
        s.yaw = yaw;
        int total = 0;
        while (total < 450) {
            int len = 1 + r.nextInt(14);
            int k = 0;
            if (r.nextInt(4) != 0) k |= F;
            if (r.nextInt(12) == 0) k |= B;
            if (r.nextInt(6) == 0) k |= L;
            if (r.nextInt(6) == 0) k |= R;
            if (r.nextInt(4) == 0) k |= J;
            if (r.nextInt(8) == 0) k |= S;
            if (r.nextInt(2) == 0) k |= P;
            if (r.nextInt(30) == 0) k |= U;
            if ((k & U) != 0) s.armSword = true;
            if (r.nextInt(25) == 0) {
                s.knock((r.nextDouble() - 0.5) * 0.8, r.nextDouble() * 0.5, (r.nextDouble() - 0.5) * 0.8, k);
                len--;
            }
            float turn = r.nextInt(3) == 0 ? (r.nextFloat() - 0.5f) * 40f : 0f;
            for (int i = 0; i < len; i++) {
                yaw += turn;
                s.rep(1, k, r.nextInt(5) == 0 ? Float.NaN : yaw);
            }
            total += len;
        }
        return s;
    }
}
