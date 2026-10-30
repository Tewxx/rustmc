package ft.mixintest;
import java.util.Map;
import net.minecraftforge.fml.relauncher.IFMLLoadingPlugin;
import org.spongepowered.asm.launch.MixinBootstrap;
import org.spongepowered.asm.mixin.Mixins;
@IFMLLoadingPlugin.Name("FTMixin")
@IFMLLoadingPlugin.MCVersion("1.8.9")
public class Plugin implements IFMLLoadingPlugin {
    public Plugin() { MixinBootstrap.init(); Mixins.addConfiguration("mixins.fttest.json"); }
    public String[] getASMTransformerClass() { return new String[0]; }
    public String getModContainerClass() { return null; }
    public String getSetupClass() { return null; }
    public void injectData(Map<String, Object> data) { }
    public String getAccessTransformerClass() { return null; }
}
