package rustmc.extract;

import java.lang.reflect.Field;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Map;
import net.minecraft.client.renderer.ItemModelMesher;
import net.minecraft.client.renderer.entity.RenderItem;
import net.minecraft.client.renderer.texture.TextureMap;
import net.minecraft.client.resources.model.ModelManager;
import net.minecraft.client.resources.model.ModelResourceLocation;
import net.minecraft.item.Item;
import net.minecraft.util.RegistrySimple;

/**
 * Item → model mapping from {@code RenderItem.registerItems}: one line per (item id, meta)
 * registration, plus the items that resolve through an {@code ItemMeshDefinition}.
 * Format: {@code id <TAB> meta <TAB> model} (meta -1 = any metadata, via a mesh definition).
 */
final class DumpItemModels {
    @SuppressWarnings("unchecked")
    static String dump() throws Exception {
        ModelManager manager = new ModelManager(new TextureMap("textures"));
        Field registry = ModelManager.class.getDeclaredField("modelRegistry");
        registry.setAccessible(true);
        registry.set(manager, new RegistrySimple());
        RenderItem render = new RenderItem(null, manager);
        ItemModelMesher mesher = render.getItemModelMesher();
        Field simple = ItemModelMesher.class.getDeclaredField("simpleShapes");
        simple.setAccessible(true);
        Field shapers = ItemModelMesher.class.getDeclaredField("shapers");
        shapers.setAccessible(true);
        List<String> lines = new ArrayList<String>();
        for (Map.Entry<Integer, ModelResourceLocation> e : ((Map<Integer, ModelResourceLocation>) simple.get(mesher)).entrySet()) {
            int key = e.getKey();
            // ItemModelMesher.getIndex: id << 16 | meta
            lines.add((key >> 16) + "\t" + (key & 65535) + "\t" + e.getValue().getResourcePath());
        }
        for (Object o : ((Map<Object, Object>) shapers.get(mesher)).keySet()) {
            Item item = (Item) o;
            lines.add(Item.getIdFromItem(item) + "\t-1\t" + Item.itemRegistry.getNameForObject(item).getResourcePath());
        }
        Collections.sort(lines, new java.util.Comparator<String>() {
            public int compare(String a, String b) {
                String[] x = a.split("\t");
                String[] y = b.split("\t");
                int c = Integer.compare(Integer.parseInt(x[0]), Integer.parseInt(y[0]));
                return c != 0 ? c : Integer.compare(Integer.parseInt(x[1]), Integer.parseInt(y[1]));
            }
        });
        StringBuilder sb = new StringBuilder();
        for (String l : lines) {
            sb.append(l).append('\n');
        }
        return sb.toString();
    }
}
