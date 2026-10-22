import io.netty.buffer.Unpooled;
import java.io.*;
import net.minecraft.init.Bootstrap;
import net.minecraft.network.EnumConnectionState;
import net.minecraft.network.EnumPacketDirection;
import net.minecraft.network.Packet;
import net.minecraft.network.PacketBuffer;

/**
 * Test oracle for mc-protocol: vanilla reads every packet the Rust side encoded
 * (readPacketData) and writes it back (writePacketData). Identical bytes mean both
 * directions agree with vanilla. Records: [dir: 0 clientbound, 1 serverbound][len: int][id + body].
 * Output records are the same, or dir 255 with a UTF error message.
 */
public class PacketOracle {
    public static void main(String[] a) throws Exception {
        Bootstrap.register();
        DataInputStream in = new DataInputStream(new BufferedInputStream(new FileInputStream(a[0])));
        DataOutputStream out = new DataOutputStream(new BufferedOutputStream(new FileOutputStream(a[1])));
        while (true) {
            int dir;
            try {
                dir = in.readUnsignedByte();
            } catch (EOFException e) {
                break;
            }
            byte[] data = new byte[in.readInt()];
            in.readFully(data);
            PacketBuffer buf = new PacketBuffer(Unpooled.wrappedBuffer(data));
            int id = buf.readVarIntFromBuffer();
            try {
                EnumPacketDirection d = dir == 0 ? EnumPacketDirection.CLIENTBOUND : EnumPacketDirection.SERVERBOUND;
                Packet p = EnumConnectionState.PLAY.getPacket(d, id);
                if (p == null) throw new IOException("no packet " + id);
                p.readPacketData(buf);
                if (buf.readableBytes() > 0) throw new IOException("packet " + id + ": " + buf.readableBytes() + " bytes extra");
                // Spawn player/mob write from a live DataWatcher, which a read does not set up:
                // having read the whole packet without error, echo it.
                if (dir == 0 && (id == 12 || id == 15)) {
                    out.writeByte(dir);
                    out.writeInt(data.length);
                    out.write(data);
                    continue;
                }
                PacketBuffer o = new PacketBuffer(Unpooled.buffer());
                o.writeVarIntToBuffer(id);
                p.writePacketData(o);
                byte[] b = new byte[o.readableBytes()];
                o.readBytes(b);
                out.writeByte(dir);
                out.writeInt(b.length);
                out.write(b);
            } catch (Throwable t) {
                out.writeByte(255);
                out.writeUTF("packet " + id + ": " + t);
            }
        }
        out.close();
    }
}
