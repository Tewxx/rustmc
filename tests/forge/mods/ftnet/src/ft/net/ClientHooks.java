package ft.net;

import ft.ScriptEvent;
import io.netty.buffer.Unpooled;
import java.util.*;
import net.minecraft.network.PacketBuffer;
import net.minecraftforge.fml.common.eventhandler.SubscribeEvent;
import net.minecraftforge.fml.common.network.FMLNetworkEvent;
import net.minecraftforge.fml.common.network.NetworkRegistry;
import net.minecraftforge.fml.common.network.internal.FMLProxyPacket;
import net.minecraftforge.fml.relauncher.Side;

/** Client-only driver half of FTNet (separate class: never loaded on the dedicated server). */
public class ClientHooks {
    @SubscribeEvent
    public void script(ScriptEvent e) {
        if (!e.step.equals("net")) return;
        FTNet.T.log("NET-03", "channels",
            "hasFtnet", NetworkRegistry.INSTANCE.hasChannel("ftnet", Side.CLIENT),
            "hasFtraw", NetworkRegistry.INSTANCE.hasChannel("ftraw", Side.CLIENT),
            "hasBogus", NetworkRegistry.INSTANCE.hasChannel("bogus", Side.CLIENT));
        FTNet.wrapper.sendToServer(new FTNet.Ping(41, "hello"));
        FTNet.raw.sendToServer(new FMLProxyPacket(new PacketBuffer(Unpooled.wrappedBuffer(new byte[] {1, 2, 3, 4, 5})), "ftraw"));
    }

    @SubscribeEvent
    public void connected(FMLNetworkEvent.ClientConnectedToServerEvent e) {
        FTNet.T.log("NET-03", "ClientConnectedToServerEvent(ftnet)", "vanilla", e.isLocal);
    }
}
