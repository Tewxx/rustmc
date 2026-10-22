package rustmc.extract;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import net.minecraft.block.Block;
import net.minecraft.block.state.IBlockState;

/**
 * `Block.BLOCK_STATE_IDS` both ways: which state each raw `id << 4 | meta` value resolves to
 * (what chunk storage reads), and the raw id of every state (what it writes).
 */
public final class DumpStateIds {
    private DumpStateIds() {}

    public static JsonObject dump() {
        JsonArray byRaw = new JsonArray();
        for (int raw = 0; raw < 4096; raw++) {
            IBlockState s = Block.BLOCK_STATE_IDS.getByValue(raw);
            if (s == null) {
                continue;
            }
            JsonObject o = new JsonObject();
            o.addProperty("raw", raw);
            o.addProperty("block", Block.getIdFromBlock(s.getBlock()));
            o.add("props", DumpBlocks.props(s));
            byRaw.add(o);
        }
        JsonArray byState = new JsonArray();
        for (Block b : Block.blockRegistry) {
            for (IBlockState s : b.getBlockState().getValidStates()) {
                JsonObject o = new JsonObject();
                o.addProperty("block", Block.getIdFromBlock(b));
                o.add("props", DumpBlocks.props(s));
                o.addProperty("raw", Block.getStateId(s));
                byState.add(o);
            }
        }
        JsonObject out = new JsonObject();
        out.add("by_raw", byRaw);
        out.add("by_state", byState);
        return out;
    }
}
