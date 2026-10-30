package ft.lifecycle2;

import ft.Trace;
import net.minecraftforge.fml.common.Loader;
import net.minecraftforge.fml.common.Mod;
import net.minecraftforge.fml.common.event.*;

/** LC-09: second mod ordered after ftlifecycle via the dependencies string; IMC sender/receiver across mods. */
@Mod(modid = "ftlifecycle2", name = "FT Lifecycle 2", version = "0.1",
     dependencies = "required-after:ftlifecycle@[1.2,2.0)", acceptableRemoteVersions = "*", clientSideOnly = true)
public class FTLifecycle2 {
    static final Trace.T T = Trace.of("lifecycle");

    public FTLifecycle2() { T.log("LC-09", "mod2.constructor"); }

    @Mod.EventHandler
    public void preInit(FMLPreInitializationEvent e) {
        T.log("LC-09", "mod2.preInit", "active", Loader.instance().activeModContainer().getModId());
        net.minecraftforge.fml.common.event.FMLInterModComms.sendMessage("ftlifecycle", "from2", "hello");
    }

    @Mod.EventHandler
    public void init(FMLInitializationEvent e) { T.log("LC-09", "mod2.init"); }

    @Mod.EventHandler
    public void postInit(FMLPostInitializationEvent e) { T.log("LC-09", "mod2.postInit"); }

    @Mod.EventHandler
    public void loadComplete(FMLLoadCompleteEvent e) { T.log("LC-09", "mod2.loadComplete"); }
}
