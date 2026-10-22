package rustmc.extract;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Random;
import net.minecraft.block.Block;
import net.minecraft.block.BlockFlowerPot;
import net.minecraft.block.BlockRedstoneComparator;
import net.minecraft.block.state.IBlockState;
import net.minecraft.item.Item;
import net.minecraft.tileentity.TileEntityComparator;
import net.minecraft.tileentity.TileEntityFlowerPot;
import net.minecraft.util.BlockPos;

/**
 * `getActualState` over random 3×3×3 neighbourhoods, for every block that overrides it.
 *
 * States are written as dense global indices (blocks in id order, each block's states in
 * `getValidStates` order), the same layout as `mc_data::StateId`. Neighbours are listed for
 * dy, dz, dx in -1..=1 (skipping the centre), dx fastest.
 */
public final class DumpActualStates {
    private DumpActualStates() {}

    static final int SCENES_PER_BLOCK = 2000;

    /** Blocks some override looks for by identity or class; biases neighbours towards them. */
    static final String[] RELATED = {
        "snow", "snow_layer", "barrier", "glass", "stained_glass", "stained_glass_pane", "glass_pane", "iron_bars",
        "fence", "nether_brick_fence", "spruce_fence", "fence_gate", "spruce_fence_gate", "cobblestone_wall",
        "oak_stairs", "stone_stairs", "stone_slab", "wooden_slab", "stone_slab2", "double_stone_slab", "hopper",
        "redstone_wire", "unpowered_repeater", "powered_repeater", "unpowered_comparator", "powered_comparator",
        "redstone_torch", "lever", "stone_button", "redstone_block", "tripwire", "tripwire_hook", "pumpkin",
        "melon_block", "wooden_door", "iron_door", "double_plant", "bed", "planks", "wool", "leaves", "log",
        "tnt", "vine", "tallgrass", "pumpkin_stem", "melon_stem", "stone", "dirt", "netherrack", "lit_pumpkin",
    };

    /**
     * Per class, the blocks its logic looks for. A share of neighbours is drawn from these so rare
     * branches (snowy grass, stems pointing at fruit, locked repeaters, wire climbing) get plenty
     * of samples.
     */
    static final Map<String, String[]> FOCUS = new HashMap<String, String[]>();
    static {
        String[] snow = {"snow", "snow_layer"};
        FOCUS.put("BlockGrass", snow);
        FOCUS.put("BlockMycelium", snow);
        FOCUS.put("BlockDirt", snow);
        FOCUS.put("BlockStem", new String[] {"pumpkin", "melon_block"});
        FOCUS.put("BlockRedstoneRepeater", new String[] {"powered_repeater", "unpowered_repeater", "powered_comparator", "unpowered_comparator"});
        FOCUS.put("BlockRedstoneWire", new String[] {"redstone_wire", "stone", "glass", "unpowered_repeater", "powered_repeater", "unpowered_comparator", "redstone_torch", "lever"});
        FOCUS.put("BlockFire", new String[] {"planks", "wool", "leaves", "log", "tnt", "stone_slab", "oak_stairs", "hopper", "snow_layer", "stone"});
        FOCUS.put("BlockTripWireHook", new String[] {"stone_slab", "wooden_slab", "oak_stairs", "hopper", "snow_layer", "stone", "glass"});
        FOCUS.put("BlockVine", new String[] {"stone", "glass", "leaves"});
        FOCUS.put("BlockFenceGate", new String[] {"cobblestone_wall"});
        FOCUS.put("BlockTripWire", new String[] {"tripwire", "tripwire_hook"});
    }

    /** Item ids a flower pot can show, favoured when picking pot contents. */
    static final int[] POT_ITEMS = {6, 31, 32, 37, 38, 39, 40, 81};

    public static JsonArray dump() {
        Map<IBlockState, Integer> index = new HashMap<IBlockState, Integer>();
        List<IBlockState> all = new ArrayList<IBlockState>();
        for (Block b : Block.blockRegistry) {
            for (IBlockState s : b.getBlockState().getValidStates()) {
                index.put(s, all.size());
                all.add(s);
            }
        }
        List<IBlockState> related = new ArrayList<IBlockState>();
        for (String name : RELATED) {
            Block b = Block.getBlockFromName(name);
            if (b == null) {
                throw new IllegalStateException("unknown block " + name);
            }
            related.addAll(b.getBlockState().getValidStates());
        }

        JsonArray out = new JsonArray();
        for (Block b : Block.blockRegistry) {
            boolean overrides;
            try {
                overrides = b.getClass().getMethod("getActualState", IBlockState.class, net.minecraft.world.IBlockAccess.class, BlockPos.class)
                        .getDeclaringClass() != Block.class;
            } catch (NoSuchMethodException e) {
                throw new IllegalStateException(e);
            }
            if (!overrides) {
                continue;
            }
            List<IBlockState> own = b.getBlockState().getValidStates();
            List<IBlockState> focus = new ArrayList<IBlockState>();
            String[] focusNames = FOCUS.get(b.getClass().getSimpleName());
            if (focusNames != null) {
                for (String name : focusNames) {
                    focus.addAll(Block.getBlockFromName(name).getBlockState().getValidStates());
                }
            }
            Random rand = new Random(Block.getIdFromBlock(b) * 7919L + 12345L);
            for (int n = 0; n < SCENES_PER_BLOCK; n++) {
                SceneWorld w = new SceneWorld();
                BlockPos centre = new BlockPos(rand.nextInt(2001) - 1000, 1 + rand.nextInt(254), rand.nextInt(2001) - 1000);
                IBlockState c = own.get(rand.nextInt(own.size()));
                w.states.put(centre, c);

                JsonArray neighbours = new JsonArray();
                JsonArray comparators = new JsonArray();
                int k = 0;
                for (int dy = -1; dy <= 1; dy++) {
                    for (int dz = -1; dz <= 1; dz++) {
                        for (int dx = -1; dx <= 1; dx++) {
                            if (dx == 0 && dy == 0 && dz == 0) {
                                continue;
                            }
                            IBlockState s;
                            int r = rand.nextInt(100);
                            if (!focus.isEmpty() && r < 25) {
                                s = focus.get(rand.nextInt(focus.size()));
                            } else if (r < 40) {
                                s = all.get(0);
                            } else if (r < 60) {
                                s = own.get(rand.nextInt(own.size()));
                            } else if (r < 80) {
                                s = related.get(rand.nextInt(related.size()));
                            } else {
                                s = all.get(rand.nextInt(all.size()));
                            }
                            BlockPos p = centre.add(dx, dy, dz);
                            w.states.put(p, s);
                            neighbours.add(Json.num(index.get(s)));
                            if (s.getBlock() instanceof BlockRedstoneComparator && rand.nextBoolean()) {
                                TileEntityComparator te = new TileEntityComparator();
                                int signal = rand.nextInt(16);
                                te.setOutputSignal(signal);
                                w.tileEntities.put(p, te);
                                JsonArray pair = new JsonArray();
                                pair.add(Json.num(k));
                                pair.add(Json.num(signal));
                                comparators.add(pair);
                            }
                            k++;
                        }
                    }
                }

                JsonObject scene = new JsonObject();
                if (b instanceof BlockFlowerPot && rand.nextInt(10) < 8) {
                    int pick = rand.nextInt(4);
                    int itemId = pick < 2 ? POT_ITEMS[rand.nextInt(POT_ITEMS.length)] : pick == 2 ? rand.nextInt(200) : 256 + rand.nextInt(200);
                    Item item = Item.getItemById(itemId);
                    if (item != null) {
                        int data = rand.nextInt(20) - 2;
                        w.tileEntities.put(centre, new TileEntityFlowerPot(item, data));
                        JsonArray pot = new JsonArray();
                        pot.add(Json.num(itemId));
                        pot.add(Json.num(data));
                        scene.add("pot", pot);
                    }
                }

                JsonArray pos = new JsonArray();
                pos.add(Json.num(centre.getX()));
                pos.add(Json.num(centre.getY()));
                pos.add(Json.num(centre.getZ()));
                scene.add("pos", pos);
                scene.addProperty("c", index.get(c));
                scene.add("n", neighbours);
                if (comparators.size() > 0) {
                    scene.add("cmp", comparators);
                }
                scene.addProperty("out", index.get(c.getBlock().getActualState(c, w, centre)));
                out.add(scene);
            }
        }
        return out;
    }
}
