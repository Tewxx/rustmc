package rustmc.extract;

import com.google.gson.*;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.util.*;
import net.minecraft.block.*;
import net.minecraft.block.state.IBlockState;
import net.minecraft.entity.item.EntityBoat;
import net.minecraft.tileentity.TileEntityPiston;
import net.minecraft.util.*;

/** Ordered geometry calls, retaining the mutable Block bounds between calls within a scene. */
final class DumpGeometry {
    static final Random RANDOM = new Random(1894701);
    static final List<IBlockState> STATES = new ArrayList<IBlockState>();
    static final Map<IBlockState, Integer> INDEX = new HashMap<IBlockState, Integer>();
    static final Map<Block, double[]> DEFAULTS = new IdentityHashMap<Block, double[]>();
    static Method SET_BOUNDS;

    static JsonArray dump() throws Exception {
        SET_BOUNDS = Block.class.getDeclaredMethod("setBlockBounds", float.class, float.class, float.class, float.class, float.class, float.class);
        SET_BOUNDS.setAccessible(true);
        for (Block b : Block.blockRegistry) {
            DEFAULTS.put(b, rawBounds(b));
            for (IBlockState s : b.getBlockState().getValidStates()) { INDEX.put(s, STATES.size()); STATES.add(s); }
        }
        GeometryWorld world = new GeometryWorld();
        JsonArray result = new JsonArray();
        for (IBlockState state : STATES) result.add(scene(world, state, false));
        for (Block b : Block.blockRegistry) {
            boolean dynamic = b.getClass().getMethod("setBlockBoundsBasedOnState", net.minecraft.world.IBlockAccess.class, BlockPos.class).getDeclaringClass() != Block.class;
            boolean sides = b.getClass().getMethod("shouldSideBeRendered", net.minecraft.world.IBlockAccess.class, BlockPos.class, EnumFacing.class).getDeclaringClass() != Block.class;
            if (!dynamic && !sides) continue;
            List<IBlockState> valid = b.getBlockState().getValidStates();
            for (int i = 0; i < 300; i++) result.add(scene(world, valid.get(RANDOM.nextInt(valid.size())), true));
        }
        return result;
    }

    static JsonObject scene(GeometryWorld world, IBlockState state, boolean neighbours) throws Exception {
        world.access.states.clear(); world.access.tileEntities.clear();
        for (Block b : Block.blockRegistry) {
            double[] v = DEFAULTS.get(b); SET_BOUNDS.invoke(b, (float)v[0], (float)v[1], (float)v[2], (float)v[3], (float)v[4], (float)v[5]);
            if (b instanceof BlockStairs) {
                Field f = BlockStairs.class.getDeclaredField("hasRaytraced"); f.setAccessible(true); f.setBoolean(b, false);
                f = BlockStairs.class.getDeclaredField("rayTracePass"); f.setAccessible(true); f.setInt(b, 0);
            }
        }
        int[] coordinates = {0, 15, 16, -1, -16, 16777215, -16777216, 29999999, -29999999};
        BlockPos pos = new BlockPos(coordinates[RANDOM.nextInt(coordinates.length)], RANDOM.nextInt(256), coordinates[RANDOM.nextInt(coordinates.length)]);
        world.access.states.put(pos, state);
        JsonObject row = new JsonObject(); row.addProperty("c", INDEX.get(state)); row.add("pos", array(pos.getX(), pos.getY(), pos.getZ()));
        JsonArray ns = new JsonArray();
        if (neighbours) for (EnumFacing side : EnumFacing.values()) for (int distance = 1; distance <= 2; distance++) {
            BlockPos p = pos.offset(side, distance);
            IBlockState s;
            int choice = RANDOM.nextInt(5);
            if (choice == 0) s = state;
            else if (choice == 1) s = state.getBlock().getBlockState().getValidStates().get(RANDOM.nextInt(state.getBlock().getBlockState().getValidStates().size()));
            else if (choice == 2) s = STATES.get(RANDOM.nextInt(STATES.size()));
            else {
                String[] related = {"stone", "glass", "stained_glass", "stone_slab", "wooden_slab", "leaves", "fence", "cobblestone_wall", "fence_gate", "oak_stairs", "quartz_stairs", "air", "barrier", "pumpkin", "portal"};
                Block block = Block.getBlockFromName(related[RANDOM.nextInt(related.length)]);
                List<IBlockState> states = block.getBlockState().getValidStates(); s = states.get(RANDOM.nextInt(states.size()));
            }
            world.access.states.put(p, s); ns.add(array(side.getIndex(), distance, INDEX.get(s)));
        }
        row.add("n", ns);
        if (state.getBlock() instanceof BlockPistonMoving && neighbours) {
            String[] carriedNames={"air","stone","glass","slime","piston","sticky_piston","piston_head","piston_extension","stone_slab","ladder","fence_gate","cactus","soul_sand","oak_stairs"};
            Block carriedBlock=Block.getBlockFromName(carriedNames[RANDOM.nextInt(carriedNames.length)]);
            List<IBlockState> valid=carriedBlock.getBlockState().getValidStates(); IBlockState carried=valid.get(RANDOM.nextInt(valid.size()));
            EnumFacing facing=EnumFacing.values()[RANDOM.nextInt(6)];boolean extending=RANDOM.nextBoolean();
            float progress=new float[]{0F,0.25F,0.5F,0.75F,1F}[RANDOM.nextInt(5)];
            TileEntityPiston piston=new TileEntityPiston(carried,facing,extending,false);
            for (String name:new String[]{"progress","lastProgress"}) {Field f=TileEntityPiston.class.getDeclaredField(name);f.setAccessible(true);f.setFloat(piston,progress);}
            world.access.tileEntities.put(pos,piston);
            row.add("piston",array(INDEX.get(carried),facing.getIndex(),Float.floatToIntBits(progress),extending?1:0));
        }
        Block b = state.getBlock(); JsonArray steps = new JsonArray();
        b.setBlockBoundsBasedOnState(world, pos); step(steps, "update", b);
        JsonObject sideStep = step(steps, "sides", b);
        for (boolean fancy : new boolean[] {false, true}) {
            for (Block block : Block.blockRegistry) if (block instanceof BlockLeaves) ((BlockLeaves)block).setGraphicsLevel(fancy);
            int flags = 0;
            for (EnumFacing side : EnumFacing.values()) if (b.shouldSideBeRendered(world, pos.offset(side), side)) flags |= 1 << side.getIndex();
            sideStep.addProperty(fancy ? "fancy" : "fast", flags);
        }
        for (Block block : Block.blockRegistry) if (block instanceof BlockLeaves) ((BlockLeaves)block).setGraphicsLevel(false);
        AxisAlignedBB selected = b.getSelectedBoundingBox(world, pos);
        step(steps, "select", b).add("box", box(selected));
        AxisAlignedBB collision = b.getCollisionBoundingBox(world, pos, state);
        step(steps, "collision", b).add("box", box(collision));
        for (int i = 0; i < 3; i++) {
            AxisAlignedBB mask = i == 0 ? new AxisAlignedBB(pos.add(-2, -2, -2), pos.add(3, 3, 3))
                : new AxisAlignedBB(pos.getX() + 0.5, pos.getY() + 0.125, pos.getZ() + 0.5, pos.getX() + 1.0, pos.getY() + 0.75, pos.getZ() + 1.0);
            if (i == 2) mask = new AxisAlignedBB(pos.getX(), pos.getY() + 0.5, pos.getZ(), pos.getX() + 1.0, pos.getY() + 1.0, pos.getZ() + 1.0);
            List<AxisAlignedBB> boxes = new ArrayList<AxisAlignedBB>();
            b.addCollisionBoxesToList(world, pos, state, mask, boxes, i == 1 ? new EntityBoat(world) : null);
            JsonObject op = step(steps, "boxes", b); op.addProperty("boat", i == 1); op.add("mask", box(mask));
            JsonArray list = new JsonArray(); for (AxisAlignedBB aabb : boxes) list.add(box(aabb)); op.add("boxes", list);
        }
        for (int i = 0; i < 13; i++) {
            double a = i < 3 ? 0.5 : RANDOM.nextDouble() * 1.5 - 0.25;
            double c = i < 3 ? 0.5 : RANDOM.nextDouble() * 1.5 - 0.25;
            Vec3 start, end;
            if (i % 3 == 0) { start = new Vec3(pos.getX() - 1.0, pos.getY() + a, pos.getZ() + c); end = new Vec3(pos.getX() + 2.0, pos.getY() + a, pos.getZ() + c); }
            else if (i % 3 == 1) { start = new Vec3(pos.getX() + a, pos.getY() + 2.0, pos.getZ() + c); end = new Vec3(pos.getX() + a, pos.getY() - 1.0, pos.getZ() + c); }
            else { start = new Vec3(pos.getX() + a, pos.getY() + c, pos.getZ() + 2.0); end = new Vec3(pos.getX() + a, pos.getY() + c, pos.getZ() - 1.0); }
            if (i == 6 || i == 7) {
                start = new Vec3(pos.getX() + RANDOM.nextDouble() * 3.0 - 1.0, pos.getY() + RANDOM.nextDouble() * 3.0 - 1.0, pos.getZ() + RANDOM.nextDouble() * 3.0 - 1.0);
                end = new Vec3(pos.getX() + RANDOM.nextDouble() * 3.0 - 1.0, pos.getY() + RANDOM.nextDouble() * 3.0 - 1.0, pos.getZ() + RANDOM.nextDouble() * 3.0 - 1.0);
            } else if (i == 8) {
                start = new Vec3(pos.getX() - 1.0, pos.getY() - 1.0, pos.getZ() - 1.0); end = new Vec3(pos.getX() + 2.0, pos.getY() + 2.0, pos.getZ() + 2.0);
            } else if (i == 9 || i == 10) {
                start = new Vec3(pos.getX() + 0.5, pos.getY() + 0.5, pos.getZ() + 0.5);
                end = i == 9 ? new Vec3(pos.getX() + 2.0, pos.getY() + 2.0, pos.getZ() + 2.0) : start;
            } else if (i == 11) {
                start = new Vec3(pos.getX(), pos.getY() + 0.5, pos.getZ() + 0.5); end = new Vec3(pos.getX() - 1.0, pos.getY() + 0.5, pos.getZ() + 0.5);
            } else if (i == 12) {
                start = new Vec3(pos.getX() + 0.5, pos.getY() + 2.0, pos.getZ() + 0.5); end = new Vec3(pos.getX() + 0.50001, pos.getY() - 1.0, pos.getZ() + 0.5);
            }
            MovingObjectPosition hit = b.collisionRayTrace(world, pos, start, end);
            JsonObject op = step(steps, "ray", b); op.add("start", array(start.xCoord, start.yCoord, start.zCoord)); op.add("end", array(end.xCoord, end.yCoord, end.zCoord));
            op.add("hit", hit == null ? JsonNull.INSTANCE : array(hit.hitVec.xCoord, hit.hitVec.yCoord, hit.hitVec.zCoord, hit.sideHit.getIndex()));
        }
        b.setBlockBoundsBasedOnState(world, pos); step(steps, "update", b);
        row.add("steps", steps); return row;
    }

    static JsonObject step(JsonArray steps, String name, Block b) {
        JsonObject op = new JsonObject(); op.addProperty("op", name); op.add("bounds", array(rawBounds(b))); steps.add(op); return op;
    }
    static double[] rawBounds(Block b) { return new double[] {b.getBlockBoundsMinX(), b.getBlockBoundsMinY(), b.getBlockBoundsMinZ(), b.getBlockBoundsMaxX(), b.getBlockBoundsMaxY(), b.getBlockBoundsMaxZ()}; }
    static JsonElement box(AxisAlignedBB b) { return b == null ? JsonNull.INSTANCE : array(b.minX, b.minY, b.minZ, b.maxX, b.maxY, b.maxZ); }
    static JsonArray array(double... values) { JsonArray a = new JsonArray(); for (double v : values) a.add(new JsonPrimitive(v)); return a; }
}
