package ft.lifecycle;

import ft.Trace;
import java.util.*;
import net.minecraftforge.fml.common.*;
import net.minecraftforge.fml.common.event.*;

/** LC-*: construction -> preInit -> init -> postInit -> loadComplete, IMC, @SidedProxy, @Mod.Instance, metadata. */
@Mod(modid = "ftlifecycle", name = "FT Lifecycle", version = "1.2.3", useMetadata = true,
     dependencies = "required-after:ftcommon", acceptableRemoteVersions = "*", clientSideOnly = true)
public class FTLifecycle {
    public static final Trace.T T = Trace.of("lifecycle");

    @Mod.Instance("ftlifecycle")
    public static FTLifecycle instance;

    @Mod.Metadata("ftlifecycle")
    public static ModMetadata metadata;

    @SidedProxy(clientSide = "ft.lifecycle.ClientProxy", serverSide = "ft.lifecycle.CommonProxy")
    public static CommonProxy proxy;

    public FTLifecycle() {
        // constructor runs before FMLConstructionEvent; @Mod.Instance is not yet injected
        T.log("LC-01", "constructor", "instanceInjected", instance != null);
    }

    @Mod.EventHandler
    public void construct(FMLConstructionEvent e) {
        T.log("LC-01", "FMLConstructionEvent", "instanceInjected", instance != null);
    }

    @Mod.EventHandler
    public void preInit(FMLPreInitializationEvent e) {
        T.log("LC-02", "FMLPreInitializationEvent",
            "instanceIsThis", instance == this,
            "proxyClass", proxy == null ? null : proxy.getClass().getName(),
            "side", e.getSide(),
            "configDir", e.getModConfigurationDirectory().getName(),
            "suggestedConfig", e.getSuggestedConfigurationFile().getName(),
            "activeContainer", Loader.instance().activeModContainer().getModId(),
            "state", Loader.instance().getLoaderState().name());
        ModMetadata m = e.getModMetadata();
        T.log("LC-07", "metadata",
            "sameAsField", m == metadata,
            "modId", m.modId, "name", m.name, "version", m.version,
            "description", m.description, "authors", m.authorList, "url", m.url, "credits", m.credits);
        ModContainer c = Loader.instance().getIndexedModList().get("ftlifecycle");
        T.log("LC-07", "container", "modId", c.getModId(), "name", c.getName(), "version", c.getVersion(),
            "displayVersion", c.getDisplayVersion(), "mcversion", c.acceptableMinecraftVersionRange() == null ? null : "present");
        proxy.preInit();
        FMLInterModComms.sendMessage("ftlifecycle", "ping", "from-preInit");
        FMLInterModComms.sendMessage("ftlifecycle", "ping2", "second");
    }

    @Mod.EventHandler
    public void init(FMLInitializationEvent e) {
        T.log("LC-03", "FMLInitializationEvent", "state", Loader.instance().getLoaderState().name());
        proxy.init();
    }

    @Mod.EventHandler
    public void imc(FMLInterModComms.IMCEvent e) {
        List<String> msgs = new ArrayList<String>();
        for (FMLInterModComms.IMCMessage m : e.getMessages())
            msgs.add(m.getSender() + ":" + m.key + "=" + (m.isStringMessage() ? m.getStringValue() : "<nonstring>"));
        T.log("LC-04", "IMCEvent", "messages", msgs);
        T.log("LC-04", "runtimeMessages", "count", FMLInterModComms.fetchRuntimeMessages(this).size());
    }

    @Mod.EventHandler
    public void postInit(FMLPostInitializationEvent e) {
        T.log("LC-05", "FMLPostInitializationEvent");
        List<String> ids = new ArrayList<String>();
        for (ModContainer c : Loader.instance().getModList()) ids.add(c.getModId());
        T.log("LC-06", "modList", "ids", ids, "isLoaded2", Loader.isModLoaded("ftlifecycle2"),
            "isLoadedBogus", Loader.isModLoaded("nosuchmod"));
        T.log("LC-06", "versions", "mc", Loader.MC_VERSION, "forge", net.minecraftforge.common.ForgeVersion.getVersion(),
            "mcString", Loader.instance().getMCVersionString());
    }

    @Mod.EventHandler
    public void loadComplete(FMLLoadCompleteEvent e) {
        T.log("LC-05", "FMLLoadCompleteEvent", "state", Loader.instance().getLoaderState().name());
    }
}
