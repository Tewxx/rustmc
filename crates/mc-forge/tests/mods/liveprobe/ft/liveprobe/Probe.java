package ft.liveprobe;

import net.minecraft.client.Minecraft;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.entity.Entity;
import net.minecraft.entity.player.EntityPlayer;
import net.minecraft.item.Item;
import net.minecraft.item.ItemStack;
import net.minecraft.network.play.client.C01PacketChatMessage;
import net.minecraft.network.Packet;
import net.minecraft.world.World;

/** Controlled compatibility fixture. Never opens a network connection or reads accounts. */
public final class Probe {
    private static final StringBuilder TRACE = new StringBuilder();
    private static EntityPlayerSP rememberedPlayer;
    private static Entity rememberedOther;
    private static World rememberedWorld;
    private static Object rememberedInventory, rememberedLoaded, rememberedPlayers;
    public static boolean cancelTick;
    public static boolean cancelWalking;

    private static void require(boolean condition, String message) {
        if (!condition) throw new AssertionError(message);
    }

    /** Checks data through the API real mods use, not the bridge's own JSON helpers. */
    public static void assertGraph(int localId, int otherId, double localX, float health,
                                   int itemId, int count, int damage) {
        Minecraft mc = Minecraft.func_71410_x();
        EntityPlayerSP player = mc.field_71439_g;
        require(player != null && mc.field_71441_e != null, "live player and world exist");
        require(player.func_145782_y() == localId, "local entity id");
        require(player.field_70165_t == localX, "position field");
        require(player.func_110143_aJ() == health, "DataWatcher-backed health getter");
        require(player.field_70170_p == mc.field_71441_e, "player world identity");
        Entity other = mc.field_71441_e.func_73045_a(otherId);
        require(other != null && other.func_145782_y() == otherId, "world entity lookup");
        require(mc.field_71441_e.field_72996_f.contains(player), "loaded entities contain player");
        require(mc.field_71441_e.field_72996_f.contains(other), "loaded entities contain remote entity");
        require(mc.field_71441_e.field_73010_i.contains(player), "player list contains local player");
        if (other instanceof EntityPlayer) {
            require(mc.field_71441_e.field_73010_i.contains(other), "player list contains remote player");
        }
        require(player.field_71071_by != null && player.field_71158_b != null, "inventory and movement objects exist");
        ItemStack held = player.field_71071_by.func_70448_g();
        require(held != null, "held stack exists");
        require(Item.func_150891_b(held.func_77973_b()) == itemId, "held item id");
        require(held.field_77994_a == count && held.func_77960_j() == damage, "stack count and damage");
        require(player.field_71174_a != null && mc.field_71442_b != null, "send queue and controller exist");
    }

    public static void remember(int otherId) {
        Minecraft mc = Minecraft.func_71410_x();
        rememberedPlayer = mc.field_71439_g;
        rememberedOther = mc.field_71441_e.func_73045_a(otherId);
        rememberedWorld = mc.field_71441_e;
        rememberedInventory = rememberedPlayer.field_71071_by;
        rememberedLoaded = rememberedWorld.field_72996_f;
        rememberedPlayers = rememberedWorld.field_73010_i;
    }

    public static void assertStable(int otherId) {
        Minecraft mc = Minecraft.func_71410_x();
        require(mc.field_71439_g == rememberedPlayer, "player object survives snapshot");
        require(mc.field_71441_e == rememberedWorld, "world object survives snapshot");
        require(mc.field_71441_e.func_73045_a(otherId) == rememberedOther, "remote object survives snapshot");
        require(mc.field_71439_g.field_71071_by == rememberedInventory, "inventory object survives snapshot");
        require(mc.field_71441_e.field_72996_f == rememberedLoaded, "loaded entity list survives snapshot");
        require(mc.field_71441_e.field_73010_i == rememberedPlayers, "player list survives snapshot");
    }

    public static void assertRemoved(int id) {
        Minecraft mc = Minecraft.func_71410_x();
        require(mc.field_71441_e.func_73045_a(id) == null, "removed id no longer resolves");
        require(!mc.field_71441_e.field_72996_f.contains(rememberedOther), "removed entity leaves loaded list");
    }

    public static void assertDisconnected() {
        Minecraft mc = Minecraft.func_71410_x();
        require(mc.field_71439_g == null && mc.field_71441_e == null, "disconnect clears game graph");
    }

    public static void mutateAndAct(int otherId) throws Exception {
        Minecraft mc = Minecraft.func_71410_x();
        EntityPlayerSP player = mc.field_71439_g;
        player.field_70177_z = 137.5f;
        player.field_70125_A = -23.25f;
        player.field_70159_w = 0.125;
        player.field_70181_x = 0.25;
        player.field_70179_y = -0.375;
        player.field_70122_E = false;
        player.func_70031_b(true);
        player.field_71071_by.field_70461_c = 7;
        player.field_71158_b.field_78902_a = -0.5f;
        player.field_71158_b.field_78900_b = 0.75f;
        player.field_71158_b.field_78901_c = true;
        player.field_71158_b.field_78899_d = true;
        player.func_71038_i();
        player.func_71165_d("liveprobe-chat");
        mc.field_71442_b.func_78764_a(player, mc.field_71441_e.func_73045_a(otherId));
        player.field_71174_a.func_147297_a(new C01PacketChatMessage("liveprobe-packet"));
        // Exercise the real mapped binary name; the generator's InnerClasses
        // simple-name metadata is a separate compile-time compatibility concern.
        Packet<?> position = (Packet<?>) Class.forName(
            "net.minecraft.network.play.client.C03PacketPlayer$C04PacketPlayerPosition")
            .getConstructor(double.class, double.class, double.class, boolean.class)
            .newInstance(1.25, 64.5, -2.75, true);
        player.field_71174_a.func_147297_a(position);
    }

    public static synchronized void resetTrace() {
        TRACE.setLength(0);
        cancelTick = false;
        cancelWalking = false;
        cancelAttack = false;
    }

    public static synchronized void clearTrace() { TRACE.setLength(0); }
    public static void setCancelTick(boolean cancel) { cancelTick = cancel; }
    public static void setCancelWalking(boolean cancel) { cancelWalking = cancel; }

    public static synchronized void mark(String value) {
        if (TRACE.length() != 0) TRACE.append(',');
        TRACE.append(value);
    }

    public static boolean cancelAttack;
    public static void setCancelAttack(boolean cancel) { cancelAttack = cancel; }

    public static net.minecraft.client.settings.KeyBinding toggle;

    /** Subscribes through the API real mods use, and registers a key binding like a mod's init. */
    public static void listen() {
        net.minecraftforge.common.MinecraftForge.EVENT_BUS.register(new Events());
        net.minecraftforge.fml.common.FMLCommonHandler.instance().bus().register(new Events.Fml());
        toggle = new net.minecraft.client.settings.KeyBinding("key.liveprobe", 19, "LiveProbe");
        net.minecraftforge.fml.client.registry.ClientRegistry.registerKeyBinding(toggle);
    }

    /** What a mod's toggle handler observes. */
    public static String keyState() {
        StringBuilder s = new StringBuilder();
        s.append(toggle.func_151463_i()).append(':').append(toggle.func_151464_g()).append(':').append(toggle.func_151470_d());
        while (toggle.func_151468_f()) s.append(":pressed");
        return s.toString();
    }

    public static final class Events {
        public static final class Fml {
            @net.minecraftforge.fml.common.eventhandler.SubscribeEvent
            public void key(net.minecraftforge.fml.common.gameevent.InputEvent.KeyInputEvent e) {
                mark("key:" + org.lwjgl.input.Keyboard.getEventKey() + ":" + org.lwjgl.input.Keyboard.getEventKeyState()
                    + ":" + org.lwjgl.input.Keyboard.isKeyDown(19));
            }
        }
        @net.minecraftforge.fml.common.eventhandler.SubscribeEvent
        public void chat(net.minecraftforge.client.event.ClientChatReceivedEvent e) {
            String text = e.message.func_150260_c();
            mark("chat:" + e.type + ":" + text);
            if (text.equals("hide me")) e.setCanceled(true);
            if (text.equals("edit me")) e.message = new net.minecraft.util.ChatComponentText("edited");
        }
        @net.minecraftforge.fml.common.eventhandler.SubscribeEvent
        public void overlay(net.minecraftforge.client.event.RenderGameOverlayEvent.Pre e) {
            if (e instanceof net.minecraftforge.client.event.RenderGameOverlayEvent.Text) {
                ((net.minecraftforge.client.event.RenderGameOverlayEvent.Text) e).left.add("LiveProbe HUD");
                mark("text");
            } else mark("pre:" + e.type);
        }
        @net.minecraftforge.fml.common.eventhandler.SubscribeEvent
        public void overlayPost(net.minecraftforge.client.event.RenderGameOverlayEvent.Post e) { mark("post:" + e.type); }
        @net.minecraftforge.fml.common.eventhandler.SubscribeEvent
        public void load(net.minecraftforge.event.world.WorldEvent.Load e) { mark("load"); }
        @net.minecraftforge.fml.common.eventhandler.SubscribeEvent
        public void unload(net.minecraftforge.event.world.WorldEvent.Unload e) { mark("unload"); }
        @net.minecraftforge.fml.common.eventhandler.SubscribeEvent
        public void join(net.minecraftforge.event.entity.EntityJoinWorldEvent e) {
            require(e.world == Minecraft.func_71410_x().field_71441_e, "join event names the live world");
            mark("join:" + e.entity.func_145782_y());
        }
        @net.minecraftforge.fml.common.eventhandler.SubscribeEvent
        public void playerTick(net.minecraftforge.fml.common.gameevent.TickEvent.PlayerTickEvent e) {
            require(e.player == Minecraft.func_71410_x().field_71439_g, "player tick names the local player");
            mark("player:" + e.phase);
        }
        @net.minecraftforge.fml.common.eventhandler.SubscribeEvent
        public void living(net.minecraftforge.event.entity.living.LivingEvent.LivingUpdateEvent e) { mark("living"); }
        @net.minecraftforge.fml.common.eventhandler.SubscribeEvent
        public void attack(net.minecraftforge.event.entity.player.AttackEntityEvent e) {
            mark("attack:" + e.target.func_145782_y());
            if (cancelAttack) e.setCanceled(true);
        }
    }

    /** RenderManager, renderViewEntity and objectMouseOver as mods read them. */
    public static void assertView(double x, double y, double z, float yaw, int targetId) {
        Minecraft mc = Minecraft.func_71410_x();
        net.minecraft.client.renderer.entity.RenderManager rm = mc.func_175598_ae();
        require(rm != null, "render manager exists");
        require(rm.field_78730_l == x && rm.field_78731_m == y && rm.field_78728_n == z, "viewer position");
        require(rm.field_78735_i == yaw, "viewer yaw");
        require(mc.func_175606_aa() == mc.field_71439_g, "render view entity is the player");
        net.minecraft.util.MovingObjectPosition mop = mc.field_71476_x;
        require(mop != null && String.valueOf(mop.field_72313_a).equals("ENTITY"), "entity hit");
        require(mop.field_72308_g == mc.field_71441_e.func_73045_a(targetId) && mc.field_147125_j == mop.field_72308_g, "pointed entity");
    }

    /** Rust's actual tick/walking continuation calls this between the Mixin hooks. */
    public static void body() { mark("body"); }
    public static synchronized String trace() { return TRACE.toString(); }
}
