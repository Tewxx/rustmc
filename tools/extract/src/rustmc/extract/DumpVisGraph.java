package rustmc.extract;

import com.google.gson.*;
import java.util.Random;
import net.minecraft.client.renderer.chunk.VisGraph;
import net.minecraft.client.renderer.chunk.SetVisibility;
import net.minecraft.util.BlockPos;
import net.minecraft.util.EnumFacing;

/** Vanilla section-visibility outputs for deterministic opaque-block layouts. */
final class DumpVisGraph {
    static JsonArray dump() {
        JsonArray all = new JsonArray();
        String[] names = {"empty", "sparse", "solid", "wall", "wall-door", "sealed-cavity", "cross-tunnels", "random"};
        for (int scene = 0; scene < names.length; scene++) {
            JsonObject row = new JsonObject(); row.addProperty("name", names[scene]);
            JsonArray opaque = new JsonArray(); VisGraph graph = new VisGraph(); Random random = new Random(83291);
            for (int y = 0; y < 16; y++) for (int z = 0; z < 16; z++) for (int x = 0; x < 16; x++) {
                boolean solid = false;
                switch (scene) {
                    case 1: solid = (x + 16*z + 256*y) % 19 == 0; break;
                    case 2: solid = true; break;
                    case 3: solid = x == 8; break;
                    case 4: solid = x == 8 && !(y == 8 && z == 8); break;
                    case 5: solid = !(x >= 4 && x <= 11 && y >= 4 && y <= 11 && z >= 4 && z <= 11); break;
                    case 6: solid = !((y == 8 && z == 8) || (x == 4 && z == 4)); break;
                    case 7: solid = random.nextInt(100) < 67; break;
                }
                if (solid) { graph.func_178606_a(new BlockPos(x,y,z)); opaque.add(new JsonPrimitive(x | (z << 4) | (y << 8))); }
            }
            SetVisibility vis = graph.computeVisibility(); long mask = 0;
            for (EnumFacing a : EnumFacing.values()) for (EnumFacing b : EnumFacing.values())
                if (vis.isVisible(a,b)) mask |= 1L << (a.ordinal() + 6*b.ordinal());
            row.add("opaque", opaque); row.addProperty("mask", mask); all.add(row);
        }
        return all;
    }
}
