package rustmc.extract;

import java.util.HashMap;
import java.util.Map;
import net.minecraft.block.material.Material;
import net.minecraft.block.state.IBlockState;
import net.minecraft.init.Blocks;
import net.minecraft.tileentity.TileEntity;
import net.minecraft.util.BlockPos;
import net.minecraft.util.EnumFacing;
import net.minecraft.world.IBlockAccess;
import net.minecraft.world.WorldType;
import net.minecraft.world.biome.BiomeGenBase;

/** A tiny `IBlockAccess` over a map of states and tile entities; everything else is air. */
final class SceneWorld implements IBlockAccess {
    final Map<BlockPos, IBlockState> states = new HashMap<BlockPos, IBlockState>();
    final Map<BlockPos, TileEntity> tileEntities = new HashMap<BlockPos, TileEntity>();
    final Map<BlockPos, BiomeGenBase> biomes = new HashMap<BlockPos, BiomeGenBase>();

    public TileEntity getTileEntity(BlockPos pos) {
        return tileEntities.get(pos);
    }

    public int getCombinedLight(BlockPos pos, int lightValue) {
        return 0;
    }

    public IBlockState getBlockState(BlockPos pos) {
        IBlockState s = states.get(pos);
        return s != null ? s : Blocks.air.getDefaultState();
    }

    /** Same definition as `World`/`ChunkCache`. */
    public boolean isAirBlock(BlockPos pos) {
        return getBlockState(pos).getBlock().getMaterial() == Material.air;
    }

    public BiomeGenBase getBiomeGenForCoords(BlockPos pos) {
        BiomeGenBase biome = biomes.get(pos);
        return biome != null ? biome : BiomeGenBase.plains;
    }

    public boolean extendedLevelsInChunkCache() {
        return false;
    }

    /** Same definition as `ChunkCache.getStrongPower`. */
    public int getStrongPower(BlockPos pos, EnumFacing direction) {
        IBlockState s = getBlockState(pos);
        return s.getBlock().getStrongPower(this, pos, s, direction);
    }

    public WorldType getWorldType() {
        return WorldType.DEFAULT;
    }
}
