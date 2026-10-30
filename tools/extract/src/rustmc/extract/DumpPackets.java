package rustmc.extract;

import com.google.common.base.Predicate;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.google.gson.JsonPrimitive;
import com.mojang.authlib.GameProfile;
import io.netty.buffer.Unpooled;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.util.*;
import net.minecraft.block.state.IBlockState;
import net.minecraft.client.Minecraft;
import net.minecraft.client.audio.ISound;
import net.minecraft.client.audio.SoundHandler;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.client.multiplayer.PlayerControllerMP;
import net.minecraft.client.network.NetHandlerPlayClient;
import net.minecraft.client.network.NetworkPlayerInfo;
import net.minecraft.client.particle.EffectRenderer;
import net.minecraft.client.renderer.EntityRenderer;
import net.minecraft.client.renderer.ItemRenderer;
import net.minecraft.client.settings.GameSettings;
import net.minecraft.client.settings.KeyBinding;
import net.minecraft.entity.Entity;
import net.minecraft.entity.item.EntityArmorStand;
import net.minecraft.entity.monster.EntityZombie;
import net.minecraft.init.Blocks;
import net.minecraft.item.Item;
import net.minecraft.item.ItemStack;
import net.minecraft.network.EnumConnectionState;
import net.minecraft.network.EnumPacketDirection;
import net.minecraft.network.NetworkManager;
import net.minecraft.network.Packet;
import net.minecraft.network.PacketBuffer;
import net.minecraft.network.play.client.C0DPacketCloseWindow;
import net.minecraft.network.play.client.C16PacketClientStatus;
import net.minecraft.profiler.Profiler;
import net.minecraft.util.AxisAlignedBB;
import net.minecraft.util.BlockPos;
import net.minecraft.util.EnumFacing;
import net.minecraft.util.MovementInputFromOptions;
import net.minecraft.world.WorldSettings;

/**
 * Records the serverbound packet stream (id + body bytes, hex) of the real client classes for scripted input.
 * Each tick follows `Minecraft.runTick`: rightClickDelayTimer, `getMouseOver`, `updateController`,
 * leftClickCounter, hotbar keys, the use/attack key handling, `sendClickBlockToController`, then the
 * player's `onUpdate` (which runs `onUpdateWalkingPlayer`). Output: golden/packets.json.
 */
final class DumpPackets {
    static final int F = 1, B = 2, L = 4, R = 8, J = 16, S = 32, P = 64;

    /** `NetworkManager` that keeps the serialised packets instead of writing them to a channel. */
    /** Keep the real join handler while avoiding rendering and window setup in the oracle. */
    static final class HeadlessMinecraft extends Minecraft {
        HeadlessMinecraft() { super(null); }
        public void loadWorld(net.minecraft.client.multiplayer.WorldClient world) { this.theWorld = world; }
        public void displayGuiScreen(net.minecraft.client.gui.GuiScreen screen) {}
    }

    static final class Capture extends NetworkManager {
        final List<String> out = new ArrayList<String>();
        Capture() { super(EnumPacketDirection.CLIENTBOUND); }
        public void sendPacket(Packet p) {
            try {
                PacketBuffer b = new PacketBuffer(Unpooled.buffer());
                b.writeVarIntToBuffer(EnumConnectionState.PLAY.getPacketId(EnumPacketDirection.SERVERBOUND, p));
                p.writePacketData(b);
                byte[] raw = new byte[b.readableBytes()];
                b.readBytes(raw);
                StringBuilder sb = new StringBuilder();
                for (byte x : raw) sb.append(String.format("%02x", x & 0xff));
                out.add(sb.toString());
            } catch (Exception e) { throw new RuntimeException(e); }
        }
        public void sendPacket(Packet p, io.netty.util.concurrent.GenericFutureListener<? extends io.netty.util.concurrent.Future<? super Void>> l, io.netty.util.concurrent.GenericFutureListener<? extends io.netty.util.concurrent.Future<? super Void>>... ls) { sendPacket(p); }
        public boolean isChannelOpen() { return true; }
        public void processReceivedPackets() {}
    }

    static final class Quiet extends net.minecraft.client.multiplayer.WorldClient {
        final Map<BlockPos, IBlockState> states = new HashMap<BlockPos, IBlockState>();
        final List<Entity> ents = new ArrayList<Entity>();
        net.minecraft.world.chunk.Chunk chunk;
        Quiet(NetHandlerPlayClient nh) {
            super(nh, new WorldSettings(0L, WorldSettings.GameType.SURVIVAL, false, false, net.minecraft.world.WorldType.DEFAULT), 0, net.minecraft.world.EnumDifficulty.NORMAL, new Profiler());
        }
        protected boolean isChunkLoaded(int x, int z, boolean allowEmpty) { return true; }
        public IBlockState getBlockState(BlockPos pos) {
            if (pos.getX() < -30000000 || pos.getZ() < -30000000 || pos.getX() >= 30000000 || pos.getZ() >= 30000000 || pos.getY() < 0 || pos.getY() >= 256) return Blocks.air.getDefaultState();
            IBlockState s = states.get(pos);
            return s != null ? s : Blocks.air.getDefaultState();
        }
        public net.minecraft.world.chunk.Chunk getChunkFromChunkCoords(int x, int z) {
            if (chunk == null) { chunk = new net.minecraft.world.chunk.Chunk(this, 0, 0); chunk.setChunkLoaded(true); }
            return chunk;
        }
        public boolean setBlockState(BlockPos pos, IBlockState s, int flags) {
            if (s.getBlock() == Blocks.air) states.remove(pos); else states.put(pos, s);
            return true;
        }
        public List<Entity> getEntitiesInAABBexcluding(Entity ex, AxisAlignedBB bb, Predicate<? super Entity> pred) {
            List<Entity> r = new ArrayList<Entity>();
            for (Entity e : ents) if (e != ex && e.getEntityBoundingBox().intersectsWith(bb) && (pred == null || pred.apply(e))) r.add(e);
            return r;
        }
    }

    static final class NoFx extends EffectRenderer {
        NoFx() { super(null, null); }
        public void addBlockHitEffects(BlockPos pos, EnumFacing side) {}
    }
    static final class NoSound extends SoundHandler {
        NoSound() { super(null, null); }
        public void playSound(ISound s) {}
    }

    static final class Tick {
        int keys;
        float yaw = Float.NaN, pitch = Float.NaN;
        int slot = -1;
        boolean attackDown, useDown;
        int attackPress, usePress;
        String op;
        int[] arg;
    }

    static final class Sc {
        final String name;
        final List<Object[]> fills = new ArrayList<Object[]>();
        final List<Object[]> ents = new ArrayList<Object[]>();
        final List<Object[]> inv = new ArrayList<Object[]>();
        final List<Tick> ticks = new ArrayList<Tick>();
        double x = 0.5, y = 64, z = 0.5;
        float yaw = 0, pitch = 0;
        String mode = "survival";
        int food = 20;
        Sc(String name) { this.name = name; }
        Sc floor() { return fill(-24, 60, -24, 24, 63, 24, "stone"); }
        Sc fill(int x1, int y1, int z1, int x2, int y2, int z2, String s) { fills.add(new Object[] {x1, y1, z1, x2, y2, z2, s}); return this; }
        Sc set(int x, int y, int z, String s) { return fill(x, y, z, x, y, z, s); }
        Sc item(int slot, String name, int count) { inv.add(new Object[] {slot, name, count, 0}); return this; }
        Sc ent(String type, int id, double x, double y, double z) { ents.add(new Object[] {type, id, x, y, z}); return this; }
        Tick t() { Tick t = new Tick(); ticks.add(t); return t; }
        Sc idle(int n) { for (int i = 0; i < n; i++) t(); return this; }
        Sc keys(int n, int keys) { for (int i = 0; i < n; i++) t().keys = keys; return this; }
        Sc look(float yaw, float pitch) { Tick t = t(); t.yaw = yaw; t.pitch = pitch; return this; }
        /** Holds the mouse buttons for n ticks; the first tick also carries the press events. */
        Sc hold(int n, int keys, boolean attack, boolean use) {
            for (int i = 0; i < n; i++) {
                Tick t = t(); t.keys = keys; t.attackDown = attack; t.useDown = use;
                if (i == 0) { t.attackPress = attack ? 1 : 0; t.usePress = use ? 1 : 0; }
            }
            return this;
        }
        Sc click(boolean attack, boolean use) { Tick t = t(); t.attackPress = attack ? 1 : 0; t.usePress = use ? 1 : 0; return this; }
        Sc slot(int s) { t().slot = s; return this; }
        Sc op(String op, int... arg) { Tick t = t(); t.op = op; t.arg = arg; return this; }
    }

    static Object alloc(Class<?> c) throws Exception { return DumpMovement.unsafe.allocateInstance(c); }
    static void put(Object o, Class<?> c, String f, Object v) throws Exception { DumpMovement.put(o, c, f, v); }
    static Object get(Object o, Class<?> c, String f) throws Exception { Field x = c.getDeclaredField(f); x.setAccessible(true); return x.get(o); }
    static Object call(Object o, Class<?> c, String m, Object... a) throws Exception {
        for (Method x : c.getDeclaredMethods()) if (x.getName().equals(m) && x.getParameterTypes().length == a.length) { x.setAccessible(true); return x.invoke(o, a); }
        throw new NoSuchMethodException(m);
    }

    static JsonArray dump() throws Exception {
        Field uf = sun.misc.Unsafe.class.getDeclaredField("theUnsafe"); uf.setAccessible(true);
        DumpMovement.unsafe = (sun.misc.Unsafe) uf.get(null);
        int n = 0;
        for (net.minecraft.block.Block b : net.minecraft.block.Block.blockRegistry) for (IBlockState s : b.getBlockState().getValidStates()) DumpMovement.INDEX.put(s, n++);
        JsonArray out = new JsonArray();
        for (Sc sc : scenarios()) out.add(run(sc));
        return out;
    }

    static JsonObject run(Sc sc) throws Exception {
        Minecraft mc = (Minecraft) alloc(HeadlessMinecraft.class);
        put(mc, Minecraft.class, "theMinecraft", mc);
        put(mc, Minecraft.class, "mcThread", Thread.currentThread());
        Capture cap = new Capture();
        NetHandlerPlayClient nh = (NetHandlerPlayClient) alloc(NetHandlerPlayClient.class);
        Quiet world = new Quiet(nh);
        JsonArray blocks = new JsonArray();
        for (Object[] f : sc.fills) {
            IBlockState s = DumpMovement.st((String) f[6]);
            int x1 = (Integer) f[0], y1 = (Integer) f[1], z1 = (Integer) f[2], x2 = (Integer) f[3], y2 = (Integer) f[4], z2 = (Integer) f[5];
            for (int x = x1; x <= x2; x++) for (int y = y1; y <= y2; y++) for (int z = z1; z <= z2; z++) world.states.put(new BlockPos(x, y, z), s);
            JsonArray a = new JsonArray();
            for (int v : new int[] {x1, y1, z1, x2, y2, z2, DumpMovement.INDEX.get(s)}) a.add(Json.num(v));
            blocks.add(a);
        }
        WorldSettings.GameType gt = sc.mode.equals("creative") ? WorldSettings.GameType.CREATIVE : WorldSettings.GameType.SURVIVAL;
        GameSettings gs = new GameSettings();
        put(gs, GameSettings.class, "mc", mc);
        gs.renderDistanceChunks = 12;
        put(mc, Minecraft.class, "gameSettings", gs);
        put(mc, Minecraft.class, "mcProfiler", new Profiler());
        put(mc, Minecraft.class, "theWorld", world);
        put(mc, Minecraft.class, "effectRenderer", alloc(NoFx.class));
        put(mc, Minecraft.class, "mcSoundHandler", alloc(NoSound.class));
        PlayerControllerMP pc = (PlayerControllerMP) alloc(PlayerControllerMP.class);
        put(pc, PlayerControllerMP.class, "currentGameType", gt);
        put(pc, PlayerControllerMP.class, "mc", mc);
        put(pc, PlayerControllerMP.class, "currentBlock", new BlockPos(-1, -1, -1));
        put(mc, Minecraft.class, "playerController", pc);
        UUID id = UUID.fromString("00000000-0000-0000-0000-000000000001");
        put(nh, NetHandlerPlayClient.class, "profile", new GameProfile(id, "packets"));
        put(nh, NetHandlerPlayClient.class, "playerInfoMap", new HashMap<UUID, NetworkPlayerInfo>());
        put(nh, NetHandlerPlayClient.class, "netManager", cap);
        put(nh, NetHandlerPlayClient.class, "gameController", mc);
        put(pc, PlayerControllerMP.class, "netClientHandler", nh);

        EntityPlayerSP p = new EntityPlayerSP(mc, world, nh, null);
        p.movementInput = new MovementInputFromOptions(gs);
        put(mc, Minecraft.class, "thePlayer", p);
        put(mc, Minecraft.class, "renderViewEntity", p);
        EntityRenderer er = (EntityRenderer) alloc(EntityRenderer.class);
        put(er, EntityRenderer.class, "mc", mc);
        put(er, EntityRenderer.class, "itemRenderer", alloc(ItemRenderer.class));
        put(mc, Minecraft.class, "entityRenderer", er);
        gt.configurePlayerCapabilities(p.capabilities);
        p.setPosition(sc.x, sc.y, sc.z);
        p.rotationYaw = sc.yaw; p.prevRotationYaw = sc.yaw; p.rotationYawHead = sc.yaw; p.prevRotationYawHead = sc.yaw;
        p.rotationPitch = sc.pitch; p.prevRotationPitch = sc.pitch;
        p.onGround = false;
        p.setEntityId(1);
        world.ents.add(p);
        p.getFoodStats().setFoodLevel(sc.food);
        for (Object[] e : sc.ents) {
            Entity en = e[0].equals("zombie") ? new EntityZombie(world) : e[0].equals("item") ? new net.minecraft.entity.item.EntityItem(world) : new EntityArmorStand(world);
            en.setEntityId((Integer) e[1]);
            en.setPosition((Double) e[2], (Double) e[3], (Double) e[4]);
            world.ents.add(en);
        }
        JsonArray inv = new JsonArray();
        for (Object[] it : sc.inv) {
            Item item = Item.getByNameOrId("minecraft:" + it[1]);
            if (item == null) throw new IllegalArgumentException("no item " + it[1]);
            ItemStack st = new ItemStack(item, (Integer) it[2], (Integer) it[3]);
            p.inventory.setInventorySlotContents((Integer) it[0], st);
            JsonArray a = new JsonArray();
            a.add(Json.num((Integer) it[0])); a.add(Json.str((String) it[1])); a.add(Json.num((Integer) it[2])); a.add(Json.num((Integer) it[3]));
            inv.add(a);
        }

        KeyBinding[] kb = {gs.keyBindForward, gs.keyBindBack, gs.keyBindLeft, gs.keyBindRight, gs.keyBindJump, gs.keyBindSneak, gs.keyBindSprint};
        Field pressed = KeyBinding.class.getDeclaredField("pressed"); pressed.setAccessible(true);
        JsonArray script = new JsonArray(), rows = new JsonArray();
        for (Tick t : sc.ticks) {
            for (int i = 0; i < 7; i++) pressed.setBoolean(kb[i], (t.keys & (1 << i)) != 0);
            if (!Float.isNaN(t.yaw)) p.rotationYaw = t.yaw;
            if (!Float.isNaN(t.pitch)) p.rotationPitch = t.pitch;
            JsonObject so = new JsonObject();
            so.addProperty("k", t.keys);
            if (!Float.isNaN(t.yaw)) so.addProperty("y", Float.floatToRawIntBits(t.yaw));
            if (!Float.isNaN(t.pitch)) so.addProperty("p", Float.floatToRawIntBits(t.pitch));
            if (t.slot >= 0) so.addProperty("s", t.slot);
            so.addProperty("ad", t.attackDown); so.addProperty("ud", t.useDown);
            so.addProperty("ap", t.attackPress); so.addProperty("up", t.usePress);
            if (t.op != null) {
                so.addProperty("op", t.op);
                JsonArray aa = new JsonArray();
                for (int v : t.arg) aa.add(Json.num(v));
                so.add("a", aa);
            }
            script.add(so);

            // Minecraft.runTick
            int rdt = (Integer) get(mc, Minecraft.class, "rightClickDelayTimer");
            if (rdt > 0) put(mc, Minecraft.class, "rightClickDelayTimer", rdt - 1);
            er.getMouseOver(1.0F);
            pc.updateController();
            int lcc = (Integer) get(mc, Minecraft.class, "leftClickCounter");
            if (lcc > 0) put(mc, Minecraft.class, "leftClickCounter", lcc - 1);
            if (t.slot >= 0) p.inventory.currentItem = t.slot;
            if ("inventory".equals(t.op)) {
                p.sendQueue.addToSendQueue(new C16PacketClientStatus(C16PacketClientStatus.EnumState.OPEN_INVENTORY_ACHIEVEMENT));
            } else if ("click".equals(t.op)) {
                pc.windowClick(t.arg[0], t.arg[1], t.arg[2], t.arg[3], p);
            } else if ("close".equals(t.op)) {
                p.sendQueue.addToSendQueue(new C0DPacketCloseWindow(p.openContainer.windowId));
            } else if ("drop".equals(t.op)) {
                p.dropOneItem(t.arg[0] != 0);
            } else if ("respawn".equals(t.op)) {
                p.respawnPlayer();
            } else if ("settings".equals(t.op)) {
                gs.renderDistanceChunks = t.arg[0];
                gs.chatVisibility = net.minecraft.entity.player.EntityPlayer.EnumChatVisibility.getEnumChatVisibility(t.arg[1]);
                gs.chatColours = t.arg[2] != 0;
                java.util.Set<net.minecraft.entity.player.EnumPlayerModelParts> parts = new java.util.HashSet<net.minecraft.entity.player.EnumPlayerModelParts>();
                for (net.minecraft.entity.player.EnumPlayerModelParts part : net.minecraft.entity.player.EnumPlayerModelParts.values()) if ((part.getPartMask() & t.arg[3]) != 0) parts.add(part);
                put(gs, GameSettings.class, "setModelParts", parts);
                gs.sendSettingsToServer();
            } else if ("join".equals(t.op)) {
                nh.handleJoinGame(new net.minecraft.network.play.server.S01PacketJoinGame(1, WorldSettings.GameType.SURVIVAL, false, 0, net.minecraft.world.EnumDifficulty.NORMAL, 20, net.minecraft.world.WorldType.DEFAULT, false));
            } else if ("mount".equals(t.op)) {
                net.minecraft.entity.item.EntityMinecartEmpty cart = new net.minecraft.entity.item.EntityMinecartEmpty(world, 0.5, 64.0, 0.5);
                cart.setEntityId(77); world.ents.add(cart); p.mountEntity(cart);
            } else if ("dismount".equals(t.op)) {
                p.mountEntity(null);
            } else if ("openwindow".equals(t.op)) {
                p.openContainer = new net.minecraft.inventory.ContainerChest(p.inventory, new net.minecraft.inventory.InventoryBasic("test", false, 27), p);
                p.openContainer.windowId = t.arg[0];
            } else if ("keepalive".equals(t.op)) {
                nh.handleKeepAlive(new net.minecraft.network.play.server.S00PacketKeepAlive(t.arg[0]));
            } else if ("confirm".equals(t.op)) {
                nh.handleConfirmTransaction(new net.minecraft.network.play.server.S32PacketConfirmTransaction(t.arg[0], (short)t.arg[1], t.arg[2] != 0));
            }
            if (p.isUsingItem()) {
                if (!t.useDown) pc.onStoppedUsingItem(p);
            } else {
                for (int i = 0; i < t.attackPress; i++) call(mc, Minecraft.class, "clickMouse");
                for (int i = 0; i < t.usePress; i++) call(mc, Minecraft.class, "rightClickMouse");
            }
            if (t.useDown && (Integer) get(mc, Minecraft.class, "rightClickDelayTimer") == 0 && !p.isUsingItem()) call(mc, Minecraft.class, "rightClickMouse");
            call(mc, Minecraft.class, "sendClickBlockToController", t.attackDown);

            if (p.ridingEntity != null) p.updateRidden(); else p.onUpdate();

            JsonArray r = new JsonArray();
            for (String h : cap.out) r.add(new JsonPrimitive(h));
            cap.out.clear();
            rows.add(r);
        }
        JsonObject o = new JsonObject();
        o.addProperty("name", sc.name);
        o.addProperty("mode", sc.mode);
        o.addProperty("food", sc.food);
        o.add("start", DumpMovement.bits(sc.x, sc.y, sc.z));
        o.addProperty("yaw", Float.floatToRawIntBits(sc.yaw));
        o.addProperty("pitch", Float.floatToRawIntBits(sc.pitch));
        o.add("blocks", blocks);
        JsonArray ea = new JsonArray();
        for (Object[] e : sc.ents) {
            JsonArray a = new JsonArray();
            a.add(Json.str((String) e[0])); a.add(Json.num((Integer) e[1])); a.add(Json.num(Double.doubleToRawLongBits((Double) e[2]))); a.add(Json.num(Double.doubleToRawLongBits((Double) e[3]))); a.add(Json.num(Double.doubleToRawLongBits((Double) e[4])));
            ea.add(a);
        }
        o.add("entities", ea);
        o.add("inventory", inv);
        o.add("script", script);
        o.add("out", rows);
        return o;
    }

    static List<Sc> scenarios() {
        List<Sc> l = new ArrayList<Sc>();
        Sc s;

        s = new Sc("walk_turn_stop").floor();
        s.idle(3).keys(20, F).look(33.5f, 0f).keys(10, F).keys(10, 0).keys(10, B).keys(8, L | F).look(-90f, 10f).idle(25);
        l.add(s);

        s = new Sc("sprint_toggles").floor();
        s.keys(5, F | P).keys(5, F).keys(3, F | P).keys(1, 0).keys(2, F).keys(1, 0).keys(2, F).keys(15, F | P).keys(5, F | P | S).keys(10, 0).keys(6, S).keys(6, 0);
        l.add(s);

        s = new Sc("jump_sprint_jump").floor();
        s.keys(40, F | P | J).keys(10, J).idle(20).keys(10, F | P | J | L);
        l.add(s);

        s = new Sc("sneak_walk").floor();
        s.keys(10, S).keys(10, S | F).keys(4, F).keys(10, S | B).idle(5);
        l.add(s);

        // Dirt at (0,64,2); the eye is at 65.62 so a pitch of ~36 degrees looks at its top face.
        s = new Sc("mine_to_completion").floor().set(0, 64, 2, "dirt");
        s.look(0f, 35f).idle(2).hold(40, 0, true, false).idle(5);
        l.add(s);

        s = new Sc("mine_cancel_and_retarget").floor().set(0, 64, 2, "dirt").set(2, 64, 2, "dirt").item(0, "iron_pickaxe", 1);
        s.look(0f, 35f).hold(4, 0, true, false).idle(3).hold(4, 0, true, false).look(-20f, 35f).hold(6, 0, true, false).idle(3).hold(30, 0, true, false);
        l.add(s);

        s = new Sc("mine_instant_stone_creative").floor().set(0, 64, 2, "stone").set(0, 64, 3, "stone").set(0, 65, 3, "stone");
        s.mode = "creative";
        s.look(0f, 35f).hold(15, 0, true, false).idle(3).hold(6, 0, true, false);
        l.add(s);

        s = new Sc("swing_air_left_click_counter").floor();
        s.look(0f, -60f).click(true, false).click(true, false).idle(12).hold(5, 0, true, false).idle(3);
        l.add(s);

        s = new Sc("place_blocks_faces").floor().set(0, 64, 3, "stone").item(0, "stone", 64).item(1, "dirt", 10);
        s.look(0f, 50f).click(false, true).idle(5).look(5f, 20f).hold(12, 0, false, true).look(-30f, 55f).idle(1).click(false, true).slot(1).look(0f, 60f).hold(9, 0, false, true).idle(5);
        l.add(s);

        s = new Sc("place_blocked_by_entity").floor().set(0, 64, 3, "stone").item(0, "stone", 64).ent("zombie", 102, 1.2, 64, 2.5);
        s.look(0f, 14f).idle(2).click(false, true).idle(5);
        l.add(s);

        s = new Sc("place_through_item").floor().set(0, 64, 3, "stone").item(0, "stone", 64).ent("item", 103, 0.95, 64, 2.5);
        s.look(0f, 14f).idle(2).click(false, true).idle(5);
        l.add(s);

        s = new Sc("place_side_face_vectors").floor().set(0, 64, 3, "stone").set(2, 64, 3, "stone").item(0, "stone", 64);
        s.x = 0.5; s.z = 0.5;
        s.look(-3f, 14f).click(false, true).idle(5).look(17f, 10f).click(false, true).idle(5).look(43f, 20f).click(false, true).idle(5);
        l.add(s);

        s = new Sc("sword_block").floor().item(0, "iron_sword", 1);
        s.hold(1, 0, false, true).hold(19, 0, false, true).idle(3).hold(5, F, false, true).idle(3);
        l.add(s);

        s = new Sc("eat_apple").floor().item(0, "apple", 3);
        s.food = 10;
        s.hold(40, 0, false, true).idle(3).hold(10, 0, false, true).idle(3);
        l.add(s);

        s = new Sc("bow_draw_release").floor().item(0, "bow", 1).item(1, "arrow", 8);
        s.hold(25, 0, false, true).idle(3).hold(3, 0, false, true).idle(3);
        l.add(s);

        s = new Sc("attack_entities").floor().ent("armor_stand", 101, 0.5, 64, 2.5).ent("zombie", 102, 3.5, 64, 0.5).item(0, "iron_sword", 1);
        s.look(0f, 5f).click(true, false).idle(2).click(true, false).idle(12).look(-90f, 7f).click(true, false).hold(6, 0, true, false).idle(3).look(0f, 5f).click(false, true).idle(5);
        l.add(s);

        s = new Sc("hotbar_swap").floor().item(0, "stone", 64).item(3, "dirt", 8).item(5, "apple", 2);
        s.idle(2).slot(3).idle(3).slot(5).keys(3, F).slot(0).slot(2).keys(2, 0).look(0f, 50f).slot(3).click(false, true).idle(4).slot(0).hold(2, 0, true, false).idle(4);
        l.add(s);

        s = new Sc("inventory_clicks").floor().item(0, "stone", 64).item(9, "dirt", 8).item(10, "apple", 2);
        s.idle(2).op("inventory").idle(2).op("click", 0, 36, 0, 0).op("click", 0, 20, 0, 0).op("click", 0, 9, 1, 0).op("click", 0, 10, 0, 1).op("click", 0, 36, 1, 2).op("click", 0, 11, 0, 0).op("click", 0, -999, 0, 0).op("click", 0, 36, 0, 4).op("close").idle(3).op("drop", 0).op("drop", 1).idle(2);
        l.add(s);

        s = new Sc("join_replies").floor();
        s.op("join").op("settings", 5, 1, 0, 21).op("settings", 12, 0, 1, 127).idle(2);
        l.add(s);

        s = new Sc("vehicle_using_item").floor().item(0, "iron_sword", 1);
        s.idle(2).op("mount").hold(8, F, false, true).idle(2).op("dismount").idle(2);
        l.add(s);

        s = new Sc("vehicle_input").floor();
        s.idle(2).op("mount").keys(3, F).keys(2, L | F | J).keys(2, S | R).look(33.5f, 8f).idle(2).op("dismount").idle(3);
        l.add(s);

        s = new Sc("server_replies").floor();
        s.op("keepalive", 0).op("keepalive", 123456).op("keepalive", -1)
         .op("confirm", 0, 17, 0).op("confirm", 0, -32768, 1)
         .op("confirm", 7, 22, 0).op("openwindow", 7).op("confirm", 7, -2, 0)
         .op("confirm", 8, 33, 0).op("confirm", 7, 34, 1).idle(2);
        l.add(s);

        s = new Sc("respawn").floor();
        s.idle(2).op("respawn").idle(2);
        l.add(s);

        s = new Sc("creative_fly_toggle").floor();
        s.mode = "creative";
        s.keys(2, J).keys(2, 0).keys(2, J).keys(3, 0).keys(10, F | J).keys(2, 0).keys(2, J).keys(5, 0).keys(2, J).keys(30, 0);
        l.add(s);

        return l;
    }
}
