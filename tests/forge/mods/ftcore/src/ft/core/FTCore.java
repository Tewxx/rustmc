package ft.core;

import ft.Trace;
import net.minecraftforge.fml.common.Mod;
import net.minecraftforge.fml.common.event.FMLInitializationEvent;

@Mod(modid="ftcore", name="FT Core", version="1.0", dependencies="required-after:ftcommon", acceptableRemoteVersions="*")
public class FTCore {
    @Mod.EventHandler public void init(FMLInitializationEvent event) {
        Trace.log("core", "CORE-01", "transformed-value", "value", Target.value());
    }
}
