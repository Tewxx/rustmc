package rustmc.extract;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.google.gson.JsonPrimitive;
import java.awt.image.BufferedImage;
import java.lang.reflect.Field;
import java.util.ArrayList;
import java.util.List;
import java.util.Random;
import javax.imageio.ImageIO;
import net.minecraft.block.Block;
import net.minecraft.block.state.IBlockState;
import net.minecraft.init.Blocks;
import net.minecraft.item.Item;
import net.minecraft.tileentity.TileEntityFlowerPot;
import net.minecraft.util.BlockPos;
import net.minecraft.world.ColorizerGrass;
import net.minecraft.world.ColorizerFoliage;
import net.minecraft.world.biome.*;

final class DumpColors {
    static void loadColorizers() throws Exception {
        for (String name : new String[] {"grass", "foliage"}) {
            BufferedImage image = ImageIO.read(DumpColors.class.getResourceAsStream("/assets/minecraft/textures/colormap/" + name + ".png"));
            int[] pixels = image.getRGB(0, 0, 256, 256, null, 0, 256);
            if (name.equals("grass")) ColorizerGrass.setGrassBiomeColorizer(pixels);
            else ColorizerFoliage.setFoliageBiomeColorizer(pixels);
        }
    }

    static JsonArray biomes() throws Exception {
        loadColorizers();
        Random random = new Random(189);
        JsonArray out = new JsonArray();
        for (BiomeGenBase biome : BiomeGenBase.getBiomeGenArray()) {
            if (biome == null) continue;
            JsonObject row = new JsonObject();
            row.addProperty("id", biome.biomeID);
            row.addProperty("name", biome.biomeName);
            row.addProperty("temperature", (double)biome.temperature);
            row.addProperty("rainfall", (double)biome.rainfall);
            row.addProperty("water", biome.waterColorMultiplier);
            BiomeGenBase effective = biome;
            if (biome instanceof BiomeGenMutated) {
                Field field = BiomeGenMutated.class.getDeclaredField("baseBiome");
                field.setAccessible(true);
                effective = (BiomeGenBase)field.get(biome);
            }
            row.addProperty("color_base", effective.biomeID);
            String mode = "Default";
            if (effective instanceof BiomeGenSwamp) mode = "Swamp";
            if (effective instanceof BiomeGenMesa) mode = "Mesa";
            if (effective instanceof BiomeGenForest) {
                Field field = BiomeGenForest.class.getDeclaredField("field_150632_aF");
                field.setAccessible(true);
                if (field.getInt(effective) == 3) mode = "RoofedForest";
            }
            row.addProperty("color_mode", mode);
            JsonArray samples = new JsonArray();
            for (int i = 0; i < 200; i++) {
                BlockPos pos = new BlockPos(random.nextInt(60000001) - 30000000,
                    i < 6 ? new int[] {0, 63, 64, 65, 255, 256}[i] : random.nextInt(256),
                    random.nextInt(60000001) - 30000000);
                JsonArray sample = position(pos);
                sample.add(new JsonPrimitive(Float.floatToIntBits(biome.getFloatTemperature(pos))));
                sample.add(new JsonPrimitive(Integer.toUnsignedLong(biome.getGrassColorAtPos(pos))));
                sample.add(new JsonPrimitive(Integer.toUnsignedLong(biome.getFoliageColorAtPos(pos))));
                samples.add(sample);
            }
            row.add("samples", samples);
            out.add(row);
        }
        return out;
    }

    static JsonArray dump() throws Exception {
        loadColorizers();
        List<BiomeGenBase> biomes = new ArrayList<BiomeGenBase>();
        for (BiomeGenBase b : BiomeGenBase.getBiomeGenArray()) if (b != null) biomes.add(b);
        Random random = new Random(18947);
        JsonArray out = new JsonArray();
        for (Block b : Block.blockRegistry) {
            for (IBlockState state : b.getBlockState().getValidStates()) {
                JsonObject row = new JsonObject();
                SceneWorld world = new SceneWorld();
                BlockPos pos = new BlockPos(random.nextInt(2001) - 1000, random.nextInt(256), random.nextInt(2001) - 1000);
                world.states.put(pos, state);
                row.addProperty("state", state.toString());
                row.add("pos", position(pos));
                JsonArray biomeIds = new JsonArray();
                for (int z = -1; z <= 1; z++) for (int x = -1; x <= 1; x++) {
                    BiomeGenBase biome = biomes.get(random.nextInt(biomes.size()));
                    world.biomes.put(pos.add(x, 0, z), biome);
                    biomeIds.add(new JsonPrimitive(biome.biomeID));
                }
                row.add("biomes", biomeIds);
                if (b == Blocks.double_plant) {
                    List<IBlockState> states = b.getBlockState().getValidStates();
                    IBlockState below = states.get(random.nextInt(states.size()));
                    world.states.put(pos.down(), below);
                    row.addProperty("below", below.toString());
                }
                if (b == Blocks.flower_pot) {
                    int item = DumpActualStates.POT_ITEMS[random.nextInt(DumpActualStates.POT_ITEMS.length)];
                    int data = random.nextInt(6);
                    world.tileEntities.put(pos, new TileEntityFlowerPot(Item.getItemById(item), data));
                    JsonArray pot = new JsonArray(); pot.add(new JsonPrimitive(item)); pot.add(new JsonPrimitive(data)); row.add("pot", pot);
                }
                row.addProperty("block_color", Integer.toUnsignedLong(b.getBlockColor()));
                row.addProperty("render_color", Integer.toUnsignedLong(b.getRenderColor(state)));
                row.addProperty("multiplier", Integer.toUnsignedLong(b.colorMultiplier(world, pos, 0)));
                out.add(row);
            }
        }
        return out;
    }

    private static JsonArray position(BlockPos pos) {
        JsonArray a = new JsonArray(); a.add(new JsonPrimitive(pos.getX())); a.add(new JsonPrimitive(pos.getY())); a.add(new JsonPrimitive(pos.getZ())); return a;
    }
}
