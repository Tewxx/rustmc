package ft;

import net.minecraftforge.fml.common.Mod;
import net.minecraftforge.fml.common.event.FMLPreInitializationEvent;

/** Shared test infrastructure: trace writer + unattended client session driver. Not itself under test. */
@Mod(modid = "ftcommon", name = "FT Common", version = "1.0", acceptableRemoteVersions = "*")
public class FTCommon {
    @Mod.EventHandler
    public void preInit(FMLPreInitializationEvent e) {
        if (e.getSide().isClient()) new ClientDriver().register();
    }
}
