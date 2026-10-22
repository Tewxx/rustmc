package rustmc.extract;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.lang.reflect.Field;
import java.lang.reflect.Modifier;
import java.util.Map;
import java.util.TreeMap;
import net.minecraft.block.Block;
import net.minecraft.block.material.Material;
import net.minecraft.block.properties.IProperty;
import net.minecraft.block.state.IBlockState;

/** Block registry: ids, names, every state with its meta, meta→state, and static block properties. */
public final class DumpBlocks {
    private DumpBlocks() {}

    public static JsonArray dump() throws Exception {
        Field hardness = Block.class.getDeclaredField("blockHardness");
        Field resistance = Block.class.getDeclaredField("blockResistance");
        hardness.setAccessible(true);
        resistance.setAccessible(true);

        JsonArray out = new JsonArray();
        for (Block b : Block.blockRegistry) {
            JsonObject o = new JsonObject();
            o.addProperty("id", Block.getIdFromBlock(b));
            o.addProperty("name", Block.blockRegistry.getNameForObject(b).toString());
            o.addProperty("class", b.getClass().getSimpleName());
            o.addProperty("material", materialName(b.getMaterial()));
            o.addProperty("opaque_cube", b.isOpaqueCube());
            o.addProperty("full_cube", b.isFullCube());
            o.addProperty("full_block", b.isFullBlock());
            o.addProperty("normal_cube", b.isNormalCube());
            o.addProperty("block_normal_cube", b.isBlockNormalCube());
            o.addProperty("translucent", b.isTranslucent());
            o.addProperty("use_neighbor_brightness", b.getUseNeighborBrightness());
            o.addProperty("light_opacity", b.getLightOpacity());
            o.addProperty("light_value", b.getLightValue());
            o.addProperty("hardness", hardness.getFloat(b));
            o.addProperty("resistance", resistance.getFloat(b));
            o.addProperty("slipperiness", b.slipperiness);
            o.addProperty("ticks_randomly", b.getTickRandomly());
            o.addProperty("render_type", b.getRenderType());
            o.addProperty("layer", b.getBlockLayer().name());
            o.addProperty("ao_light_value", b.getAmbientOcclusionLightValue());

            JsonArray bounds = new JsonArray();
            for (double d : new double[] {b.getBlockBoundsMinX(), b.getBlockBoundsMinY(), b.getBlockBoundsMinZ(),
                    b.getBlockBoundsMaxX(), b.getBlockBoundsMaxY(), b.getBlockBoundsMaxZ()}) {
                bounds.add(Json.num(d));
            }
            o.add("default_bounds", bounds);
            o.add("default_state", props(b.getDefaultState()));

            JsonArray states = new JsonArray();
            for (IBlockState s : b.getBlockState().getValidStates()) {
                JsonObject so = new JsonObject();
                so.addProperty("meta", b.getMetaFromState(s));
                so.add("props", props(s));
                states.add(so);
            }
            o.add("states", states);

            JsonArray fromMeta = new JsonArray();
            for (int m = 0; m < 16; m++) {
                try {
                    fromMeta.add(props(b.getStateFromMeta(m)));
                } catch (RuntimeException e) {
                    fromMeta.add(Json.str("!" + e.getClass().getSimpleName()));
                }
            }
            o.add("state_from_meta", fromMeta);
            out.add(o);
        }
        return out;
    }

    @SuppressWarnings({"rawtypes", "unchecked"})
    static JsonObject props(IBlockState s) {
        Map<String, String> sorted = new TreeMap<String, String>();
        for (Map.Entry<IProperty, Comparable> e : s.getProperties().entrySet()) {
            sorted.put(e.getKey().getName(), e.getKey().getName(e.getValue()));
        }
        JsonObject o = new JsonObject();
        for (Map.Entry<String, String> e : sorted.entrySet()) {
            o.addProperty(e.getKey(), e.getValue());
        }
        return o;
    }

    /** `Material` has no names; recover the static field that holds this instance. */
    static String materialName(Material m) throws IllegalAccessException {
        for (Field f : Material.class.getFields()) {
            if (Modifier.isStatic(f.getModifiers()) && f.get(null) == m) {
                return f.getName();
            }
        }
        return "?";
    }
}
