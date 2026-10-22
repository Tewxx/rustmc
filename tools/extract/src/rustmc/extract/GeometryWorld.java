package rustmc.extract;

import net.minecraft.block.state.IBlockState;
import net.minecraft.profiler.Profiler;
import net.minecraft.tileentity.TileEntity;
import net.minecraft.util.BlockPos;
import net.minecraft.world.World;
import net.minecraft.world.WorldProviderSurface;
import net.minecraft.world.WorldSettings;
import net.minecraft.world.WorldType;
import net.minecraft.world.chunk.IChunkProvider;
import net.minecraft.world.storage.SaveHandlerMP;
import net.minecraft.world.storage.WorldInfo;

/** The real block APIs require World even when they only read states. No chunks, IO or ticking. */
final class GeometryWorld extends World {
    final SceneWorld access = new SceneWorld();
    GeometryWorld() {
        super(new SaveHandlerMP(), new WorldInfo(new WorldSettings(0L, WorldSettings.GameType.SURVIVAL,
            false, false, WorldType.DEFAULT), "geometry"), new WorldProviderSurface(), new Profiler(), true);
    }
    protected IChunkProvider createChunkProvider() { return null; }
    protected int getRenderDistanceChunks() { return 8; }
    public IBlockState getBlockState(BlockPos pos) { return access.getBlockState(pos); }
    public TileEntity getTileEntity(BlockPos pos) { return access.getTileEntity(pos); }
}
