package ft.lifecycle;

public class CommonProxy {
    public void preInit() { FTLifecycle.T.log("LC-03", "proxy.preInit", "impl", getClass().getSimpleName()); }
    public void init() { FTLifecycle.T.log("LC-03", "proxy.init", "impl", getClass().getSimpleName()); }
}
