package ft.mixintest;
import ft.Trace;
import net.minecraftforge.fml.common.Mod;
import net.minecraftforge.fml.common.event.FMLInitializationEvent;
@Mod(modid="ftmixin", name="FT Mixin", version="1.0", dependencies="required-after:ftcommon", acceptableRemoteVersions="*")
public class FTMixin {
    @Mod.EventHandler public void init(FMLInitializationEvent event) {
        Trace.log("mixin", "MIXIN-01", "transformed-value", "value", new Target().value());
    }
}
