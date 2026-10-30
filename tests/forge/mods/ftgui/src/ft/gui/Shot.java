package ft.gui;

import net.minecraft.client.Minecraft;
import net.minecraft.util.ScreenShotHelper;

/** Saves the current framebuffer to <gameDir>/screenshots/<name> for the screenshot-diff tier. */
final class Shot {
    private Shot() {}

    static void take(String name) {
        Minecraft mc = Minecraft.getMinecraft();
        ScreenShotHelper.saveScreenshot(mc.mcDataDir, name, mc.displayWidth, mc.displayHeight, mc.getFramebuffer());
    }
}
