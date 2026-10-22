import java.io.*;
import java.util.*;
import net.minecraft.block.state.IBlockState;
import net.minecraft.block.Block;
import net.minecraft.entity.EnumCreatureType;
import net.minecraft.init.Bootstrap;
import net.minecraft.profiler.Profiler;
import net.minecraft.util.BlockPos;
import net.minecraft.util.IProgressUpdate;
import net.minecraft.world.*;
import net.minecraft.world.biome.BiomeGenBase;
import net.minecraft.world.chunk.Chunk;
import net.minecraft.world.chunk.IChunkProvider;
import net.minecraft.world.chunk.storage.ExtendedBlockStorage;
import net.minecraft.world.gen.ChunkProviderGenerate;
import net.minecraft.world.gen.layer.GenLayer;
import net.minecraft.world.gen.layer.IntCache;
import net.minecraft.world.storage.WorldInfo;

/**
 * Test oracle: runs vanilla 1.8.9 worldgen from the MCP classes and dumps the results so the
 * Rust port can be compared byte for byte. Not shipped; used only by tests.
 *
 *   biomes <seed> <x> <z> <w> <h>          -> two int arrays (genBiomes, biomeIndex)
 *   chunks <seed> <x0> <z0> <x1> <z1>      -> provideChunk output for each chunk
 *   populated <seed> <x0> <z0> <x1> <z1>   -> loads the area (+1 ring) like ChunkProviderServer
 *                                              and dumps the inner chunks after population
 */
public class Oracle {
    static class OWorld extends World {
        OWorld(WorldInfo info) {
            super(null, info, WorldProvider.getProviderForDimension(0), new Profiler(), false);
            this.mapStorage = new net.minecraft.world.storage.MapStorage(null);
            this.provider.registerWorld(this);
            this.chunkProvider = this.createChunkProvider();
        }
        protected IChunkProvider createChunkProvider() {
            return new Provider(this);
        }
        protected int getRenderDistanceChunks() {
            return 8;
        }
    }

    static class Provider implements IChunkProvider {
        final World world;
        final ChunkProviderGenerate gen;
        final Map<Long, Chunk> chunks = new HashMap<Long, Chunk>();

        Provider(World w) {
            world = w;
            gen = new ChunkProviderGenerate(w, w.getSeed(), true, "");
        }
        static long key(int x, int z) { return ((long) x & 0xffffffffL) | (((long) z & 0xffffffffL) << 32); }
        public boolean chunkExists(int x, int z) { return chunks.containsKey(key(x, z)); }
        public Chunk provideChunk(int x, int z) {
            Chunk c = chunks.get(key(x, z));
            if (c == null) {
                c = gen.provideChunk(x, z);
                chunks.put(key(x, z), c);
                c.onChunkLoad();
                c.populateChunk(this, this, x, z);
            }
            return c;
        }
        public Chunk provideChunk(BlockPos p) { return provideChunk(p.getX() >> 4, p.getZ() >> 4); }
        public void populate(IChunkProvider p, int x, int z) {
            Chunk c = provideChunk(x, z);
            if (!c.isTerrainPopulated()) {
                c.func_150809_p();
                gen.populate(p, x, z);
                c.setChunkModified();
            }
        }
        public boolean populateChunk(IChunkProvider p, Chunk c, int x, int z) { return gen.populateChunk(p, c, x, z); }
        public boolean saveChunks(boolean a, IProgressUpdate b) { return true; }
        public boolean unloadQueuedChunks() { return false; }
        public boolean canSave() { return false; }
        public String makeString() { return "oracle"; }
        public List<BiomeGenBase.SpawnListEntry> getPossibleCreatures(EnumCreatureType t, BlockPos p) { return gen.getPossibleCreatures(t, p); }
        public BlockPos getStrongholdGen(World w, String n, BlockPos p) { return null; }
        public int getLoadedChunkCount() { return chunks.size(); }
        public void recreateStructures(Chunk c, int x, int z) {}
        public void saveExtraData() {}
    }

    static void writeChunk(DataOutputStream out, Chunk c, boolean light) throws IOException {
        out.writeInt(c.xPosition);
        out.writeInt(c.zPosition);
        ExtendedBlockStorage[] secs = c.getBlockStorageArray();
        for (int sy = 0; sy < 16; sy++) {
            ExtendedBlockStorage s = secs[sy];
            for (int i = 0; i < 4096; i++) {
                int x = i & 15, z = (i >> 4) & 15, y = i >> 8;
                int v = 0;
                if (s != null) {
                    IBlockState st = s.get(x, y, z);
                    v = Block.getIdFromBlock(st.getBlock()) << 4 | st.getBlock().getMetaFromState(st);
                }
                out.writeShort(v);
            }
        }
        out.write(c.getBiomeArray());
        if (light) {
            for (int sy = 0; sy < 16; sy++) {
                ExtendedBlockStorage s = secs[sy];
                for (int i = 0; i < 4096; i++) {
                    int x = i & 15, z = (i >> 4) & 15, y = i >> 8;
                    int sl = s == null ? 15 : s.getExtSkylightValue(x, y, z);
                    int bl = s == null ? 0 : s.getExtBlocklightValue(x, y, z);
                    out.writeByte(sl << 4 | bl);
                }
            }
        }
    }

    public static void main(String[] a) throws Exception {
        Bootstrap.register();
        String mode = a[0];
        long seed = Long.parseLong(a[1]);
        int x0 = Integer.parseInt(a[2]), z0 = Integer.parseInt(a[3]);
        int x1 = Integer.parseInt(a[4]), z1 = Integer.parseInt(a[5]);
        String outPath = a[6];
        DataOutputStream out = new DataOutputStream(new BufferedOutputStream(new FileOutputStream(outPath)));
        if (mode.equals("biomes")) {
            GenLayer[] l = GenLayer.initializeAllBiomeGenerators(seed, WorldType.DEFAULT, "");
            for (int k = 0; k < 2; k++) {
                IntCache.resetIntCache();
                int[] v = l[k].getInts(x0, z0, x1, z1);
                for (int i = 0; i < x1 * z1; i++) out.writeInt(v[i]);
            }
        } else {
            WorldSettings ws = new WorldSettings(seed, WorldSettings.GameType.SURVIVAL, true, false, WorldType.DEFAULT);
            OWorld w = new OWorld(new WorldInfo(ws, "oracle"));
            Provider p = (Provider) w.getChunkProvider();
            if (mode.equals("chunks")) {
                for (int x = x0; x <= x1; x++)
                    for (int z = z0; z <= z1; z++)
                        writeChunk(out, p.gen.provideChunk(x, z), false);
            } else {
                for (int x = x0 - 1; x <= x1 + 1; x++)
                    for (int z = z0 - 1; z <= z1 + 1; z++)
                        p.provideChunk(x, z);
                for (int x = x0; x <= x1; x++)
                    for (int z = z0; z <= z1; z++)
                        writeChunk(out, p.provideChunk(x, z), mode.equals("populated-light"));
            }
        }
        out.close();
    }
}
