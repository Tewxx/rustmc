package ft.gui;

import ft.ScriptEvent;
import ft.Trace;
import java.util.*;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.GuiMainMenu;
import net.minecraft.client.gui.GuiScreen;
import net.minecraft.client.settings.GameSettings;
import net.minecraft.client.settings.KeyBinding;
import net.minecraftforge.client.event.GuiOpenEvent;
import net.minecraftforge.client.event.GuiScreenEvent;
import net.minecraftforge.client.event.RenderGameOverlayEvent;
import net.minecraftforge.common.MinecraftForge;
import net.minecraftforge.fml.client.registry.ClientRegistry;
import net.minecraftforge.fml.common.FMLCommonHandler;
import net.minecraftforge.fml.common.Mod;
import net.minecraftforge.fml.common.event.FMLInitializationEvent;
import net.minecraftforge.fml.common.event.FMLPreInitializationEvent;
import net.minecraftforge.fml.common.eventhandler.EventPriority;
import net.minecraftforge.fml.common.eventhandler.SubscribeEvent;
import net.minecraftforge.fml.common.gameevent.InputEvent;
import org.lwjgl.input.Keyboard;

/**
 * GUI-*: GuiScreen subclass (buttons, text field, Gui.drawRect/drawGradientRect/FontRenderer/GlStateManager/Tessellator),
 * GuiOpenEvent (observe, replace, cancel), GuiScreenEvent order, KeyBinding via ClientRegistry + KeyInputEvent.
 */
@Mod(modid = "ftgui", name = "FT GUI", version = "1.0", dependencies = "required-after:ftcommon",
     acceptableRemoteVersions = "*", clientSideOnly = true)
public class FTGui {
    static final Trace.T T = Trace.of("gui");
    static KeyBinding key;
    static final List<String> openLog = new ArrayList<String>();
    static final List<String> screenEvents = new ArrayList<String>();
    static boolean recordScreenEvents = false;
    static boolean shotRequested = false;
    static boolean hudShotRequested = false;
    private int openStage = 0;

    /** A screen the GuiOpenEvent handler replaces with FTScreen. */
    public static class ReplacedScreen extends GuiScreen {}
    /** A screen the GuiOpenEvent handler cancels. */
    public static class CancelledScreen extends GuiScreen {}

    @Mod.EventHandler
    public void preInit(FMLPreInitializationEvent e) {
        MinecraftForge.EVENT_BUS.register(this);
        FMLCommonHandler.instance().bus().register(this);
        key = new KeyBinding("key.ft.test", Keyboard.KEY_G, "key.categories.ft");
        ClientRegistry.registerKeyBinding(key);
        GameSettings gs = Minecraft.getMinecraft().gameSettings;
        T.log("GUI-05", "keybinding-registered",
            "desc", key.getKeyDescription(), "category", key.getKeyCategory(), "default", key.getKeyCodeDefault(),
            "code", key.getKeyCode(), "keyName", Keyboard.getKeyName(key.getKeyCode()),
            "inGameSettings", Arrays.asList(gs.keyBindings).contains(key), "totalBindings", gs.keyBindings.length,
            "inStatic", KeyBinding.getKeybinds().contains(key));
    }

    @Mod.EventHandler
    public void init(FMLInitializationEvent e) {
        T.log("GUI-05", "keybinding-init", "totalBindings", Minecraft.getMinecraft().gameSettings.keyBindings.length);
    }

    @SubscribeEvent(priority = EventPriority.HIGHEST)
    public void guiOpen(GuiOpenEvent e) {
        String from = Minecraft.getMinecraft().currentScreen == null ? "null" : Minecraft.getMinecraft().currentScreen.getClass().getSimpleName();
        String to = e.gui == null ? "null" : e.gui.getClass().getSimpleName();
        String r = "observe";
        if (e.gui instanceof ReplacedScreen) { e.gui = new FTScreen(); r = "replaced"; }
        else if (e.gui instanceof CancelledScreen) { e.setCanceled(true); r = "cancelled"; }
        // main menu / open-world transitions happen before the script runs: record them too
        if (openLog.size() < 16) openLog.add(from + "->" + to + ":" + r);
        T.log("GUI-02", "GuiOpenEvent", "from", from, "to", to, "action", r, "cancelable", e.isCancelable());
    }

    @SubscribeEvent
    public void screenEvent(GuiScreenEvent e) {
        if (!recordScreenEvents) return;
        String n = e.getClass().getName();
        if (screenEvents.size() < 8) screenEvents.add(n.substring(n.lastIndexOf('.') + 1));
    }

    @SubscribeEvent
    public void script(ScriptEvent e) {
        Minecraft mc = Minecraft.getMinecraft();
        if (e.step.equals("frame_probe")) {
            hudShotRequested = true;
        } else if (e.step.equals("gui_open")) {
            // 1) a screen the handler replaces, 2) one it cancels
            mc.displayGuiScreen(new CancelledScreen());
            T.log("GUI-02", "after-cancelled", "current", name(mc.currentScreen));
            recordScreenEvents = true;
            mc.displayGuiScreen(new ReplacedScreen());
            T.log("GUI-02", "after-replaced", "current", name(mc.currentScreen));
        } else if (e.step.equals("gui_interact")) {
            T.log("GUI-03", "screen-event-order", "first", screenEvents);
            recordScreenEvents = false;
            if (mc.currentScreen instanceof FTScreen) ((FTScreen) mc.currentScreen).scripted();
            shotRequested = true;
        } else if (e.step.equals("gui_close")) {
            mc.displayGuiScreen(null);
            T.log("GUI-02", "closed", "current", name(mc.currentScreen));
        } else if (e.step.equals("keypress")) {
            // Drive the key through the real KeyBinding state machine and the FML input event.
            KeyBinding.onTick(key.getKeyCode());
            FMLCommonHandler.instance().bus().post(new InputEvent.KeyInputEvent());
            KeyBinding.unPressAllKeys();
        }
    }

    private static String name(GuiScreen s) { return s == null ? "null" : s.getClass().getSimpleName(); }

    @SubscribeEvent
    public void keyInput(InputEvent.KeyInputEvent e) {
        boolean p = key.isPressed();
        T.log("GUI-05", "KeyInputEvent", "ourKeyPressed", p, "secondPoll", key.isPressed(), "isKeyDown", key.isKeyDown());
    }

    @SubscribeEvent
    public void hudShot(RenderGameOverlayEvent.Post e) {
        if (hudShotRequested && e.type == RenderGameOverlayEvent.ElementType.ALL) {
            hudShotRequested = false;
            Shot.take("ft-hud.png");
        }
    }
}
