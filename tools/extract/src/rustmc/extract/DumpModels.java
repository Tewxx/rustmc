package rustmc.extract;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.util.Map;
import net.minecraft.block.Block;
import net.minecraft.block.state.IBlockState;
import net.minecraft.client.renderer.BlockModelShapes;
import net.minecraft.client.resources.model.ModelResourceLocation;

/** No GL or ModelManager needed: the real state mapper runs independently of baking. */
final class DumpModels {
    static JsonArray dump() {
        Map<IBlockState, ModelResourceLocation> locations = new BlockModelShapes(null)
            .getBlockStateMapper().putAllStateModelLocations();
        JsonArray result = new JsonArray();
        for (Block block : Block.blockRegistry) {
            for (IBlockState state : block.getBlockState().getValidStates()) {
                JsonObject row = new JsonObject();
                row.addProperty("state", state.toString());
                ModelResourceLocation loc = locations.get(state);
                if (loc != null) {
                    row.addProperty("name", loc.getResourceDomain() + ":" + loc.getResourcePath());
                    row.addProperty("variant", loc.getVariant());
                }
                result.add(row);
            }
        }
        return result;
    }
}
