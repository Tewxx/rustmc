package spike;
import java.io.File;
import java.util.List;
import net.minecraft.launchwrapper.ITweaker;
import net.minecraft.launchwrapper.LaunchClassLoader;
import org.spongepowered.asm.launch.MixinTweaker;
import org.spongepowered.asm.mixin.Mixins;
public class Tweaker implements ITweaker {
    private final MixinTweaker mixin = new MixinTweaker();
    public void acceptOptions(List<String> args, File gameDir, File assetsDir, String profile) {
        mixin.acceptOptions(args, gameDir, assetsDir, profile);
        Mixins.addConfiguration("mixins.fttest.json");
    }
    public void injectIntoClassLoader(LaunchClassLoader loader) { mixin.injectIntoClassLoader(loader); }
    public String getLaunchTarget() { return "driverapp.TargetMain"; }
    public String[] getLaunchArguments() { return new String[0]; }
}
