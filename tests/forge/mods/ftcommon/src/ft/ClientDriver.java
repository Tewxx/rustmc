package ft;

import net.minecraft.client.Minecraft;
import net.minecraftforge.common.MinecraftForge;
import net.minecraftforge.fml.common.eventhandler.SubscribeEvent;
import net.minecraftforge.fml.common.gameevent.TickEvent;

/** Unattended session script: counts client ticks since world entry and posts ScriptEvent steps. */
public class ClientDriver {
    private static final Object[][] STEPS = {
        {1, "joined"}, {5, "tick_probe"}, {10, "frame_probe"}, {20, "gui_open"}, {24, "gui_interact"}, {30, "gui_close"},
        {35, "keypress"}, {40, "chat"}, {50, "net"}, {60, "interact"}, {70, "entity"}, {80, "thirdperson"},
        {100, "end"},
    };
    private final Trace.T t = Trace.of("ftcommon");
    private int ticks = -1;
    private int idx = 0;
    private int endTick = -1;

    public void register() {
        net.minecraftforge.fml.common.FMLCommonHandler.instance().bus().register(this);
        final long limit = Long.getLong("ft.timeout.s", 240L) * 1000L;
        Thread wd = new Thread("ft-watchdog") {
            public void run() {
                try { Thread.sleep(limit); } catch (InterruptedException e) { return; }
                System.err.println("[ft] watchdog: session did not finish in " + limit + " ms");
                Runtime.getRuntime().halt(3);
            }
        };
        wd.setDaemon(true);
        wd.start();
    }

    @SubscribeEvent
    public void onTick(TickEvent.ClientTickEvent e) {
        if (e.phase != TickEvent.Phase.END) return;
        Minecraft mc = Minecraft.getMinecraft();
        if (mc.theWorld == null || mc.thePlayer == null) return;
        ticks++;
        while (idx < STEPS.length && ticks >= (Integer) STEPS[idx][0]) {
            String step = (String) STEPS[idx][1];
            idx++;
            t.log("DRV-01", "step", "step", step);
            MinecraftForge.EVENT_BUS.post(new ScriptEvent(step, ticks));
            if (step.equals("end")) endTick = ticks + 5;
        }
        if (endTick >= 0 && ticks >= endTick) {
            t.log("DRV-01", "shutdown");
            mc.shutdown();
        }
    }
}
