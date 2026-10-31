package rustmc.forge;

import net.minecraftforge.fml.common.FMLCommonHandler;
import net.minecraftforge.fml.common.gameevent.TickEvent;

/** Entry point for Rust-owned client tick boundaries. */
public final class EventBridge {
    public static void clientTick(boolean start) {
        FMLCommonHandler.instance().bus().post(new TickEvent.ClientTickEvent(
            start ? TickEvent.Phase.START : TickEvent.Phase.END));
    }
}
