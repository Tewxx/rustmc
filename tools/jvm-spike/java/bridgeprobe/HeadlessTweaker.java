package bridgeprobe;
import java.io.File;
import java.util.List;
import net.minecraft.launchwrapper.ITweaker;
import net.minecraft.launchwrapper.LaunchClassLoader;
import net.minecraftforge.fml.common.launcher.FMLTweaker;
/** Delegate Forge's actual client bootstrap, but replace the game entry point with a headless probe. */
public final class HeadlessTweaker implements ITweaker {
    private final FMLTweaker forge = new FMLTweaker();
    public void acceptOptions(List<String> args, File gameDir, File assetsDir, String profile) { forge.acceptOptions(args, gameDir, assetsDir, profile); }
    public void injectIntoClassLoader(LaunchClassLoader loader) { forge.injectIntoClassLoader(loader); }
    public String getLaunchTarget() { forge.getLaunchTarget(); return "driverapp.ShadowMain"; }
    public String[] getLaunchArguments() { return forge.getLaunchArguments(); }
}
