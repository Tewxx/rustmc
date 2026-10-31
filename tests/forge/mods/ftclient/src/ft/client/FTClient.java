package ft.client;

import ft.ScriptEvent;
import ft.Trace;
import java.util.*;
import net.minecraft.client.Minecraft;
import net.minecraft.entity.Entity;
import net.minecraft.entity.EntityLivingBase;
import net.minecraftforge.client.event.*;
import net.minecraftforge.common.MinecraftForge;
import net.minecraftforge.event.entity.EntityJoinWorldEvent;
import net.minecraftforge.event.entity.living.LivingEvent;
import net.minecraftforge.event.entity.player.PlayerInteractEvent;
import net.minecraftforge.event.world.WorldEvent;
import net.minecraftforge.fml.common.FMLCommonHandler;
import net.minecraftforge.fml.common.Mod;
import net.minecraftforge.fml.common.event.FMLPreInitializationEvent;
import net.minecraftforge.fml.common.eventhandler.EventPriority;
import net.minecraftforge.fml.common.eventhandler.SubscribeEvent;
import net.minecraftforge.fml.common.gameevent.TickEvent;
import net.minecraftforge.fml.common.network.FMLNetworkEvent;
import net.minecraftforge.fml.relauncher.ReflectionHelper;

/**
 * CL-*: client tick / render tick phases, per-frame render event order (HUD ElementType order, FOV, world-last,
 * living/player render), world + entity join, living update, mouse, interact, reflection on SRG names.
 *
 * Render events fire a nondeterministic number of times, so the first frame after a ScriptEvent is recorded
 * as ONE ordered line (a list of event tokens) instead of one line per event.
 */
@Mod(modid = "ftclient", name = "FT Client", version = "1.0", dependencies = "required-after:ftcommon",
     acceptableRemoteVersions = "*", clientSideOnly = true)
public class FTClient {
    static final Trace.T T = Trace.of("client");

    // ---- frame recorder ----
    private String armedName = null;
    private List<String> frame = null;
    private boolean armedWait = false;

    // ---- tick-phase probe ----
    private boolean probing = false;
    private final List<String> tickTokens = new ArrayList<String>();
    private boolean renderOpen = false, clientOpen = false, interleaved = false, renderUnpaired = false;
    private int probeTicks = 0;

    private int livingLeft = 0;
    private final List<String> joined = new ArrayList<String>();

    @Mod.EventHandler
    public void preInit(FMLPreInitializationEvent e) {
        MinecraftForge.EVENT_BUS.register(this);
        FMLCommonHandler.instance().bus().register(this);
    }

    // ---------------------------------------------------------------- script
    @SubscribeEvent
    public void onScript(ScriptEvent e) {
        Minecraft mc = Minecraft.getMinecraft();
        if (e.step.equals("joined")) {
            T.log("CL-03", "joined-summary", "entityJoins", joined);
        } else if (e.step.equals("tick_probe")) {
            probing = true;
        } else if (e.step.equals("frame_probe")) {
            armedName = "hud-frame";
            armedWait = true;
        } else if (e.step.equals("entity")) {
            livingLeft = 3;
        } else if (e.step.equals("thirdperson")) {
            mc.gameSettings.thirdPersonView = 1;
            armedName = "thirdperson-frame";
            armedWait = true;
        } else if (e.step.equals("interact")) {
            try {
                // reflection on an SRG name: rightClickMouse == func_147121_ag
                java.lang.reflect.Method m = ReflectionHelper.findMethod(Minecraft.class, mc,
                    new String[] {"rightClickMouse", "func_147121_ag"});
                T.log("CL-08", "findMethod", "found", m != null, "name", m == null ? null : "present");
                m.invoke(mc);
            } catch (Throwable t) {
                T.log("CL-08", "reflect-failed", "type", t.getClass().getName());
            }
            T.log("CL-07", "after-rightClick", "interactEvents", interacts);
        } else if (e.step.equals("end")) {
            mc.gameSettings.thirdPersonView = 0;
            T.log("CL-09", "mouse-events-with-input", "count", mouseEvents);
        }
    }

    // ---------------------------------------------------------------- tick phases (FML bus)
    @SubscribeEvent
    public void clientTick(TickEvent.ClientTickEvent e) {
        if (probing) {
            if (e.phase == TickEvent.Phase.START) {
                if (clientOpen || renderOpen) interleaved = true;
                clientOpen = true;
            } else {
                if (!clientOpen || renderOpen) interleaved = true;
                clientOpen = false;
                tickTokens.add("C");
                if (++probeTicks == 3) {
                    probing = false;
                    T.log("CL-01", "client-tick-phases", "type", e.type, "side", e.side, "tickPairs", tickTokens.size(),
                        "startBeforeEnd", !interleaved, "renderInsideTick", interleaved, "renderUnpaired", renderUnpaired);
                }
            }
        }
    }

    @SubscribeEvent
    public void renderTick(TickEvent.RenderTickEvent e) {
        if (probing) {
            if (e.phase == TickEvent.Phase.START) {
                if (renderOpen || clientOpen) interleaved = true;
                renderOpen = true;
            } else {
                if (!renderOpen) renderUnpaired = true;
                renderOpen = false;
            }
        }
        if (e.phase == TickEvent.Phase.START) {
            if (armedWait && armedName != null) { frame = new ArrayList<String>(); armedWait = false; }
            record("RenderTick.START");
        } else {
            record("RenderTick.END");
            if (frame != null) {
                T.log("CL-02", armedName, "order", frame, "renderTickTimeInRange", e.renderTickTime >= 0 && e.renderTickTime <= 1);
                frame = null;
                armedName = null;
            }
        }
    }

    private void record(String s) { if (frame != null) frame.add(s); }

    // ---------------------------------------------------------------- per-frame render events (Forge bus)
    @SubscribeEvent
    public void overlay(RenderGameOverlayEvent e) {
        String n = e.getClass().getName();
        record("Overlay." + n.substring(n.lastIndexOf('$') + 1) + ":" + e.type);
    }

    @SubscribeEvent public void fov(FOVUpdateEvent e) { record("FOVUpdate"); }
    @SubscribeEvent public void worldLast(RenderWorldLastEvent e) { record("RenderWorldLast"); }
    @SubscribeEvent public void hand(RenderHandEvent e) { record("RenderHand"); }
    @SubscribeEvent public void highlight(DrawBlockHighlightEvent e) { record("DrawBlockHighlight"); }
    @SubscribeEvent public void view(EntityViewRenderEvent e) {
        String n = e.getClass().getName();
        record("View." + n.substring(n.lastIndexOf('$') + 1));
    }
    @SubscribeEvent public void livingPre(RenderLivingEvent.Pre e) { record("RenderLiving.Pre:" + e.entity.getClass().getSimpleName()); }
    @SubscribeEvent public void livingPost(RenderLivingEvent.Post e) { record("RenderLiving.Post:" + e.entity.getClass().getSimpleName()); }
    @SubscribeEvent public void playerPre(RenderPlayerEvent.Pre e) { record("RenderPlayer.Pre:" + e.entityPlayer.getName()); }
    @SubscribeEvent public void playerPost(RenderPlayerEvent.Post e) { record("RenderPlayer.Post:" + e.entityPlayer.getName()); }

    // ---------------------------------------------------------------- world / entity
    @SubscribeEvent
    public void connected(FMLNetworkEvent.ClientConnectedToServerEvent e) {
        T.log("CL-03", "ClientConnectedToServerEvent", "isLocal", e.isLocal, "connectionType", e.connectionType);
    }

    @SubscribeEvent
    public void worldLoad(WorldEvent.Load e) {
        T.log("CL-03", "WorldEvent.Load", "remote", e.world.isRemote, "provider", e.world.provider.getClass().getSimpleName());
    }

    @SubscribeEvent
    public void entityJoin(EntityJoinWorldEvent e) {
        joined.add(e.entity.getClass().getSimpleName() + (e.world.isRemote ? "@remote" : "@local"));
    }

    @SubscribeEvent
    public void living(LivingEvent.LivingUpdateEvent e) {
        if (livingLeft > 0 && e.entityLiving == Minecraft.getMinecraft().thePlayer) {
            livingLeft--;
            T.log("CL-05", "LivingUpdateEvent", "entity", e.entityLiving.getClass().getSimpleName(),
                "isCancelable", e.isCancelable(), "remote", e.entityLiving.worldObj.isRemote);
        }
    }

    private final List<String> interacts = new ArrayList<String>();

    @SubscribeEvent
    public void interact(PlayerInteractEvent e) {
        interacts.add(e.action + ":" + e.entityPlayer.getName());
    }

    private int mouseEvents = 0;

    @SubscribeEvent
    public void mouse(MouseEvent e) {
        if (e.button != -1 || e.dwheel != 0) mouseEvents++;
    }
}
