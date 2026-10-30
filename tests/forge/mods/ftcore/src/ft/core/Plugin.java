package ft.core;

import java.util.Map;
import net.minecraftforge.fml.relauncher.IFMLLoadingPlugin;

@IFMLLoadingPlugin.Name("FTCore")
@IFMLLoadingPlugin.MCVersion("1.8.9")
@IFMLLoadingPlugin.TransformerExclusions({"ft.core.Plugin", "ft.core.Transformer"})
public class Plugin implements IFMLLoadingPlugin {
    public String[] getASMTransformerClass() { return new String[] {"ft.core.Transformer"}; }
    public String getModContainerClass() { return null; }
    public String getSetupClass() { return null; }
    public void injectData(Map<String, Object> data) { }
    public String getAccessTransformerClass() { return null; }
}
