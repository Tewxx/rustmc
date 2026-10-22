package rustmc.extract;
import com.google.gson.*;
import net.minecraft.world.*;
import net.minecraft.world.storage.*;
import net.minecraft.world.chunk.IChunkProvider;
import net.minecraft.world.biome.BiomeGenBase;
import net.minecraft.profiler.Profiler;
import net.minecraft.entity.item.EntityBoat;
import net.minecraft.util.*;

public final class DumpSky {
    static final class SkyWorld extends World {
        BiomeGenBase biome=BiomeGenBase.plains;
        SkyWorld(){super(new SaveHandlerMP(),new WorldInfo(new WorldSettings(0,WorldSettings.GameType.CREATIVE,false,false,WorldType.DEFAULT),"sky"),new WorldProviderSurface(),new Profiler(),true);provider.registerWorld(this);}
        protected IChunkProvider createChunkProvider(){return null;}
        protected int getRenderDistanceChunks(){return 8;}
        public BiomeGenBase getBiomeGenForCoords(BlockPos p){return biome;}
    }
    public static JsonArray dump() {
        SkyWorld world=new SkyWorld();EntityBoat eye=new EntityBoat(world);eye.setPosition(8.5,70.,8.5);BlockPos pos=new BlockPos(eye);
        JsonArray out=new JsonArray();
        for(BiomeGenBase biome:BiomeGenBase.getBiomeGenArray()) {if(biome==null||biome.biomeID==9)continue;world.biome=biome;
            for(long time:new long[]{0,1000,6000,11000,12000,13000,18000,23000,23999})for(float partial:new float[]{0,0.5F}) {
                world.getWorldInfo().setWorldTime(time);JsonObject o=new JsonObject();o.addProperty("time",time);o.addProperty("partial",partial);o.addProperty("temperature",biome.getFloatTemperature(pos));
                float angle=world.getCelestialAngle(partial);o.addProperty("angle",angle);o.add("sky",vec(world.getSkyColor(eye,partial)));o.add("fog",vec(world.getFogColor(partial)));o.add("cloud",vec(world.getCloudColour(partial)));o.addProperty("stars",world.getStarBrightness(partial));o.addProperty("moon",world.getMoonPhase());
                float[] sunrise=world.provider.calcSunriseSunsetColors(angle,partial);JsonArray a=new JsonArray();if(sunrise!=null)for(float f:sunrise)a.add(new JsonPrimitive(f));o.add("sunrise",a);out.add(o);
            }
        }return out;
    }
    static JsonArray vec(Vec3 v){JsonArray a=new JsonArray();a.add(new JsonPrimitive(v.xCoord));a.add(new JsonPrimitive(v.yCoord));a.add(new JsonPrimitive(v.zCoord));return a;}
}
