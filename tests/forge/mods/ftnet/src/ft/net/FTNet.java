package ft.net;

import ft.Trace;
import io.netty.buffer.ByteBuf;
import io.netty.buffer.Unpooled;
import java.util.*;
import net.minecraft.network.PacketBuffer;
import net.minecraftforge.fml.common.FMLCommonHandler;
import net.minecraftforge.fml.common.Mod;
import net.minecraftforge.fml.common.event.FMLInitializationEvent;
import net.minecraftforge.fml.common.event.FMLPreInitializationEvent;
import net.minecraftforge.fml.common.eventhandler.SubscribeEvent;
import net.minecraftforge.fml.common.network.FMLEventChannel;
import net.minecraftforge.fml.common.network.FMLNetworkEvent;
import net.minecraftforge.fml.common.network.NetworkRegistry;
import net.minecraftforge.fml.common.network.internal.FMLProxyPacket;
import net.minecraftforge.fml.common.network.simpleimpl.SimpleNetworkWrapper;
import net.minecraftforge.fml.common.network.ByteBufUtils;
import net.minecraftforge.fml.common.network.simpleimpl.IMessage;
import net.minecraftforge.fml.common.network.simpleimpl.IMessageHandler;
import net.minecraftforge.fml.common.network.simpleimpl.MessageContext;
import net.minecraftforge.fml.relauncher.Side;

/**
 * NET-*: SimpleNetworkWrapper + event-driven FMLEventChannel custom payloads, round trip against the Forge test
 * server (this same jar is installed on the server; the server half replies and broadcasts chat).
 * Trace group "net" (client side); the server writes its own trace to the server dir (not compared).
 */
@Mod(modid = "ftnet", name = "FT Net", version = "1.0", dependencies = "required-after:ftcommon",
     acceptableRemoteVersions = "*")
public class FTNet {
    static final Trace.T T = Trace.of("net");
    public static SimpleNetworkWrapper wrapper;
    public static FMLEventChannel raw;

    // ---------------- simple-impl messages
    public static class Ping implements IMessage {
        public int n; public String s;
        public Ping() {}
        public Ping(int n, String s) { this.n = n; this.s = s; }
        public void fromBytes(ByteBuf b) { n = b.readInt(); s = ByteBufUtils.readUTF8String(b); }
        public void toBytes(ByteBuf b) { b.writeInt(n); ByteBufUtils.writeUTF8String(b, s); }
    }

    public static class Pong implements IMessage {
        public int n; public String s; public String player;
        public Pong() {}
        public Pong(int n, String s, String player) { this.n = n; this.s = s; this.player = player; }
        public void fromBytes(ByteBuf b) { n = b.readInt(); s = ByteBufUtils.readUTF8String(b); player = ByteBufUtils.readUTF8String(b); }
        public void toBytes(ByteBuf b) { b.writeInt(n); ByteBufUtils.writeUTF8String(b, s); ByteBufUtils.writeUTF8String(b, player); }
    }

    public static class ServerPingHandler implements IMessageHandler<Ping, Pong> {
        public Pong onMessage(Ping m, MessageContext ctx) {
            final String name = ctx.getServerHandler().playerEntity.getName();
            Trace.log("net-server", "NET-01", "server-received", "n", m.n, "s", m.s, "side", ctx.side, "player", name);
            ctx.getServerHandler().playerEntity.getServerForPlayer().addScheduledTask(new Runnable() {
                public void run() { ServerSide.broadcastTestChat(); }
            });
            return new Pong(m.n + 1, m.s.toUpperCase(Locale.ROOT), name);   // reply via return value
        }
    }

    public static class ClientPongHandler implements IMessageHandler<Pong, IMessage> {
        public IMessage onMessage(Pong m, MessageContext ctx) {
            T.log("NET-01", "client-received-pong", "n", m.n, "s", m.s, "player", m.player, "side", ctx.side,
                "hasServerHandler", ctx.getServerHandler() != null);
            return null;
        }
    }

    @Mod.EventHandler
    public void preInit(FMLPreInitializationEvent e) {
        wrapper = NetworkRegistry.INSTANCE.newSimpleChannel("ftnet");
        wrapper.registerMessage(ServerPingHandler.class, Ping.class, 0, Side.SERVER);
        wrapper.registerMessage(ClientPongHandler.class, Pong.class, 1, Side.CLIENT);
        raw = NetworkRegistry.INSTANCE.newEventDrivenChannel("ftraw");
        raw.register(this);
    }

    @Mod.EventHandler
    public void init(FMLInitializationEvent e) {
        if (e.getSide().isClient()) {
            ClientHooks h = new ClientHooks();
            FMLCommonHandler.instance().bus().register(h);
            net.minecraftforge.common.MinecraftForge.EVENT_BUS.register(h);
        }
    }

    // ---------------- raw channel (both sides)
    @SubscribeEvent
    public void serverRaw(FMLNetworkEvent.ServerCustomPacketEvent e) {
        ByteBuf in = e.packet.payload();
        byte[] data = new byte[in.readableBytes()];
        in.readBytes(data);
        Trace.log("net-server", "NET-02", "server-raw", "len", data.length, "channel", e.packet.channel(), "target", e.packet.getTarget());
        byte[] rev = new byte[data.length];
        for (int i = 0; i < data.length; i++) rev[i] = data[data.length - 1 - i];
        raw.sendTo(new FMLProxyPacket(new PacketBuffer(Unpooled.wrappedBuffer(rev)), "ftraw"),
            ((net.minecraft.network.NetHandlerPlayServer) e.handler).playerEntity);
    }

    @SubscribeEvent
    public void clientRaw(FMLNetworkEvent.ClientCustomPacketEvent e) {
        ByteBuf in = e.packet.payload();
        byte[] data = new byte[in.readableBytes()];
        in.readBytes(data);
        T.log("NET-02", "client-raw-received", "bytes", Arrays.toString(data), "channel", e.packet.channel(),
            "target", e.packet.getTarget());
    }

    /** Server-only helper kept in its own class so the client never links server classes. */
    public static class ServerSide {
        public static void broadcastTestChat() {
            net.minecraft.server.MinecraftServer s = net.minecraft.server.MinecraftServer.getServer();
            s.getConfigurationManager().sendChatMsg(new net.minecraft.util.ChatComponentText("plain server message"));
            s.getConfigurationManager().sendChatMsg(new net.minecraft.util.ChatComponentText("[cancel-me] hidden message"));
            s.getConfigurationManager().sendChatMsg(new net.minecraft.util.ChatComponentText("[replace-me] original message"));
        }
    }
}
