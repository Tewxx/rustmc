package ft.shadow;
import java.util.Map;
import net.minecraftforge.fml.relauncher.IFMLLoadingPlugin;
import org.spongepowered.asm.launch.MixinBootstrap;
import org.spongepowered.asm.mixin.Mixins;
@IFMLLoadingPlugin.Name("FTShadow")
@IFMLLoadingPlugin.MCVersion("1.8.9")
public class Plugin implements IFMLLoadingPlugin {
    public Plugin() {
        MixinBootstrap.init();
        if (!"0".equals(System.getenv("RUSTMC_SHADOW_MIXIN"))) Mixins.addConfiguration("mixins.ftshadow.json");
    }
    public String[] getASMTransformerClass() { return new String[0]; }
    public String getModContainerClass() { return null; }
    public String getSetupClass() { return null; }
    public void injectData(Map<String,Object> data) { }
    public String getAccessTransformerClass() { return null; }
}
