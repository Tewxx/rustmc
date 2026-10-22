package rustmc.extract;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.lang.reflect.Field;
import java.lang.reflect.Modifier;
import net.minecraft.block.material.Material;

/** Every `Material` constant in declaration order, with its flags. */
public final class DumpMaterials {
    private DumpMaterials() {}

    public static JsonArray dump() throws IllegalAccessException {
        JsonArray out = new JsonArray();
        for (Field f : Material.class.getFields()) {
            if (!Modifier.isStatic(f.getModifiers()) || f.getType() != Material.class) {
                continue;
            }
            Material m = (Material) f.get(null);
            JsonObject o = new JsonObject();
            o.addProperty("name", f.getName());
            o.addProperty("class", m.getClass().getSimpleName());
            o.addProperty("liquid", m.isLiquid());
            o.addProperty("solid", m.isSolid());
            o.addProperty("blocks_light", m.blocksLight());
            o.addProperty("blocks_movement", m.blocksMovement());
            o.addProperty("can_burn", m.getCanBurn());
            o.addProperty("replaceable", m.isReplaceable());
            o.addProperty("opaque", m.isOpaque());
            o.addProperty("tool_not_required", m.isToolNotRequired());
            o.addProperty("mobility", m.getMaterialMobility());
            o.addProperty("map_color", m.getMaterialMapColor().colorIndex);
            out.add(o);
        }
        return out;
    }
}
