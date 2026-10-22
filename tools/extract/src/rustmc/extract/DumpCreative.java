package rustmc.extract;

import java.io.*;
import java.lang.reflect.Field;
import java.util.*;
import net.minecraft.creativetab.CreativeTabs;
import net.minecraft.item.*;
import net.minecraft.block.Block;
import net.minecraft.client.renderer.*;
import net.minecraft.client.renderer.entity.RenderItem;
import net.minecraft.client.renderer.texture.TextureMap;
import net.minecraft.client.resources.model.*;

/** Catalogue order and item model identifiers from the actual 1.8.9 registries. */
public final class DumpCreative {
    @SuppressWarnings("unchecked")
    public static void write(File out) throws Exception {
        ModelManager manager = new ModelManager((TextureMap)null) {
            @Override public IBakedModel getModel(ModelResourceLocation location) { return null; }
        };
        ItemModelMesher mesher = new RenderItem(null, manager).getItemModelMesher();
        Field shapesField = ItemModelMesher.class.getDeclaredField("simpleShapes"); shapesField.setAccessible(true);
        Field shapersField = ItemModelMesher.class.getDeclaredField("shapers"); shapersField.setAccessible(true);
        Map<Integer,ModelResourceLocation> shapes = (Map<Integer,ModelResourceLocation>)shapesField.get(mesher);
        Map<Item,ItemMeshDefinition> shapers = (Map<Item,ItemMeshDefinition>)shapersField.get(mesher);
        PrintWriter writer = new PrintWriter(new OutputStreamWriter(new FileOutputStream(out), "UTF-8"));
        for (CreativeTabs tab : CreativeTabs.creativeTabArray) {
            if (tab.getTabIndex()==11) continue;
            List<ItemStack> stacks = new ArrayList<ItemStack>();
            if (tab.getTabIndex()==5) {
                for (Item item : Item.itemRegistry) if (item.getCreativeTab()!=null) item.getSubItems(item,tab,stacks);
            } else tab.displayAllReleventItems(stacks);
            for (ItemStack stack : stacks) {
                Item item=stack.getItem(); int id=Item.getIdFromItem(item), meta=stack.getMetadata();
                ModelResourceLocation model=shapes.get((id<<16)|(stack.isItemStackDamageable()?0:meta));
                if (model==null && shapers.containsKey(item)) model=shapers.get(item).getModelLocation(stack);
                String name=stack.getDisplayName().replace('\t',' ').replace('\n',' ');
                writer.println(tab.getTabIndex()+"\t"+id+"\t"+meta+"\t"+stack.getMaxStackSize()+"\t"+Item.itemRegistry.getNameForObject(item)+"\t"+name+"\t"+(model==null?"":model.getResourcePath())+"\t"+Math.max(0,Block.getIdFromBlock(Block.getBlockFromItem(item))));
            }
        }
        writer.close();
        System.out.println("wrote "+out);
    }
}
