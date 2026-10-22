package rustmc.extract;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.lang.reflect.Field;
import java.lang.reflect.Modifier;
import java.util.Map;
import java.util.TreeMap;
import net.minecraft.block.Block;
import net.minecraft.block.BlockLeaves;
import net.minecraft.block.material.Material;
import net.minecraft.block.properties.IProperty;
import net.minecraft.block.properties.PropertyBool;
import net.minecraft.block.properties.PropertyDirection;
import net.minecraft.block.properties.PropertyEnum;
import net.minecraft.block.properties.PropertyInteger;
import net.minecraft.block.state.IBlockState;

/** Block registry: ids, names, every state with its meta, meta→state, and static block properties. */
/** Floats are written widened to double (exact), so readers recover the same f32 bits. */
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
            o.addProperty("hardness", (double) hardness.getFloat(b));
            o.addProperty("resistance", (double) resistance.getFloat(b));
            o.addProperty("slipperiness", (double) b.slipperiness);
            o.addProperty("ticks_randomly", b.getTickRandomly());
            o.addProperty("render_type", b.getRenderType());
            o.addProperty("layer", b.getBlockLayer().name());
            o.addProperty("ao_light_value", (double) b.getAmbientOcclusionLightValue());
            // Leaves are the only blocks whose opacity/layer follow the graphics setting
            // (RenderGlobal.loadRenderers calls setGraphicsLevel); record the fancy values too.
            if (b instanceof BlockLeaves) {
                ((BlockLeaves) b).setGraphicsLevel(true);
                o.addProperty("opaque_cube_fancy", b.isOpaqueCube());
                o.addProperty("layer_fancy", b.getBlockLayer().name());
                ((BlockLeaves) b).setGraphicsLevel(false);
            } else {
                o.addProperty("opaque_cube_fancy", b.isOpaqueCube());
                o.addProperty("layer_fancy", b.getBlockLayer().name());
            }

            o.addProperty("unlocalized_name", b.getUnlocalizedName());
            o.addProperty("collidable", b.isCollidable());
            o.addProperty("enable_stats", b.getEnableStats());
            o.addProperty("mobility", b.getMobilityFlag());
            o.addProperty("has_tile_entity", b.hasTileEntity());
            o.addProperty("can_provide_power", b.canProvidePower());
            o.addProperty("comparator_override", b.hasComparatorInputOverride());
            o.addProperty("requires_updates", b.requiresUpdates());
            o.addProperty("can_spawn_in", b.canSpawnInBlock());

            Material mt = b.getMaterial();
            JsonObject mat = new JsonObject();
            mat.addProperty("liquid", mt.isLiquid());
            mat.addProperty("solid", mt.isSolid());
            mat.addProperty("blocks_light", mt.blocksLight());
            mat.addProperty("blocks_movement", mt.blocksMovement());
            mat.addProperty("can_burn", mt.getCanBurn());
            mat.addProperty("replaceable", mt.isReplaceable());
            mat.addProperty("opaque", mt.isOpaque());
            mat.addProperty("tool_not_required", mt.isToolNotRequired());
            mat.addProperty("mobility", mt.getMaterialMobility());
            mat.addProperty("map_color", mt.getMaterialMapColor().colorIndex);
            o.add("material_props", mat);

            JsonObject sound = new JsonObject();
            sound.addProperty("name", b.stepSound.soundName);
            sound.addProperty("volume", (double) b.stepSound.volume);
            sound.addProperty("frequency", (double) b.stepSound.frequency);
            sound.addProperty("break", b.stepSound.getBreakSound());
            sound.addProperty("step", b.stepSound.getStepSound());
            sound.addProperty("place", b.stepSound.getPlaceSound());
            o.add("sound", sound);

            JsonArray propDefs = new JsonArray();
            for (IProperty p : b.getBlockState().getProperties()) {
                propDefs.add(propertyDef(p));
            }
            o.add("properties", propDefs);

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

    /** Name, kind and allowed values in `getAllowedValues()` order (which fixes state order). */
    @SuppressWarnings({"rawtypes", "unchecked"})
    static JsonObject propertyDef(IProperty p) {
        JsonObject o = new JsonObject();
        o.addProperty("name", p.getName());
        String kind = p instanceof PropertyBool ? "bool"
                : p instanceof PropertyInteger ? "int"
                : p instanceof PropertyDirection ? "direction"
                : p instanceof PropertyEnum ? "enum" : "?";
        o.addProperty("kind", kind);
        o.addProperty("value_class", p.getValueClass().getSimpleName());
        JsonArray values = new JsonArray();
        for (Object v : p.getAllowedValues()) {
            values.add(Json.str(p.getName((Comparable) v)));
        }
        o.add("values", values);
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
