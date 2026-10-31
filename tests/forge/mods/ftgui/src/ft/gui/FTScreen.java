package ft.gui;

import java.io.IOException;
import java.util.*;
import net.minecraft.client.gui.*;
import net.minecraft.client.renderer.GlStateManager;
import net.minecraft.client.renderer.Tessellator;
import net.minecraft.client.renderer.WorldRenderer;
import net.minecraft.client.renderer.vertex.DefaultVertexFormats;
import org.lwjgl.input.Keyboard;

/** The test GUI: exercises widgets, input handlers and every draw path a mod UI normally uses. */
public class FTScreen extends GuiScreen {
    private GuiButton ok, cancel;
    private GuiTextField field;
    private int frames = 0;
    private boolean metricsLogged = false;

    @Override
    public void initGui() {
        this.buttonList.clear();
        ok = new GuiButton(1, this.width / 2 - 100, this.height / 2 - 10, 98, 20, "OK");
        cancel = new GuiButton(2, this.width / 2 + 2, this.height / 2 - 10, 98, 20, "Cancel");
        this.buttonList.add(ok);
        this.buttonList.add(cancel);
        field = new GuiTextField(0, this.fontRendererObj, this.width / 2 - 100, this.height / 2 + 20, 200, 20);
        field.setMaxStringLength(16);
        FTGui.T.log("GUI-01", "initGui", "width", width, "height", height, "buttons", buttonList.size(),
            "ok", ok.xPosition + "," + ok.yPosition + "," + ok.getButtonWidth(),
            "cancel", cancel.xPosition + "," + cancel.yPosition,
            "field", field.xPosition + "," + field.yPosition + "," + field.getWidth(),
            "pauses", doesGuiPauseGame());
    }

    @Override
    public void drawScreen(int mouseX, int mouseY, float partialTicks) {
        this.drawDefaultBackground();
        Gui.drawRect(this.width / 2 - 110, this.height / 2 - 40, this.width / 2 + 110, this.height / 2 + 50, 0xC0101010);
        this.drawGradientRect(this.width / 2 - 100, this.height / 2 - 36, this.width / 2 + 100, this.height / 2 - 20, 0xFFFF0000, 0xFF0000FF);
        this.drawCenteredString(this.fontRendererObj, "rustmc forge test", this.width / 2, this.height / 2 - 34, 0xFFFFFF);
        this.drawString(this.fontRendererObj, "left aligned §cred§r plain", this.width / 2 - 100, this.height / 2 + 45, 0xAAAAAA);
        this.fontRendererObj.drawStringWithShadow("shadow", this.width / 2 + 60, this.height / 2 + 45, 0xFFFF55);

        // raw tessellator quad under GlStateManager transforms
        GlStateManager.pushMatrix();
        GlStateManager.translate(this.width / 2f + 70, this.height / 2f - 70, 0f);
        GlStateManager.scale(2f, 2f, 1f);
        GlStateManager.disableTexture2D();
        GlStateManager.enableBlend();
        GlStateManager.tryBlendFuncSeparate(770, 771, 1, 0);
        Tessellator tess = Tessellator.getInstance();
        WorldRenderer wr = tess.getWorldRenderer();
        wr.begin(7, DefaultVertexFormats.POSITION_COLOR);
        wr.pos(0, 0, 0).color(255, 0, 0, 255).endVertex();
        wr.pos(0, 12, 0).color(0, 255, 0, 255).endVertex();
        wr.pos(12, 12, 0).color(0, 0, 255, 255).endVertex();
        wr.pos(12, 0, 0).color(255, 255, 0, 128).endVertex();
        tess.draw();
        GlStateManager.enableTexture2D();
        GlStateManager.disableBlend();
        GlStateManager.color(1f, 1f, 1f, 1f);
        GlStateManager.popMatrix();

        field.drawTextBox();
        super.drawScreen(mouseX, mouseY, partialTicks); // buttons

        if (!metricsLogged) {
            metricsLogged = true;
            net.minecraft.client.gui.FontRenderer f = this.fontRendererObj;
            FTGui.T.log("GUI-04", "font-metrics",
                "FONT_HEIGHT", f.FONT_HEIGHT,
                "w(Hello)", f.getStringWidth("Hello"),
                "w(Hello World!)", f.getStringWidth("Hello World!"),
                "w(bold)", f.getStringWidth("§lBold"),
                "w(color-code)", f.getStringWidth("§cred§r plain"),
                "w(unicode)", f.getStringWidth("café ü 中"),
                "trim(12)", f.trimStringToWidth("Hello World!", 12),
                "trimReverse(12)", f.trimStringToWidth("Hello World!", 12, true),
                "wrap(40)", f.listFormattedStringToWidth("the quick brown fox jumps", 40),
                "getCharWidth(i)", f.getCharWidth('i'), "getCharWidth(W)", f.getCharWidth('W'),
                "unicodeFlag", f.getUnicodeFlag(), "bidi", f.getBidiFlag());
        }
        if (FTGui.shotRequested && ++frames > 2) {
            FTGui.shotRequested = false;
            Shot.take("ft-gui.png");
            FTGui.T.log("GUI-06", "screenshot-taken", "file", "ft-gui.png");
        }
    }

    /** Scripted "user" input, run from the client tick (real input events can't be injected portably). */
    void scripted() {
        try {
            // click the OK button in its middle
            mouseClicked(ok.xPosition + 5, ok.yPosition + 5, 0);
            // click the text field, then type
            mouseClicked(field.xPosition + 5, field.yPosition + 5, 0);
            keyTyped('h', Keyboard.KEY_H);
            keyTyped('i', Keyboard.KEY_I);
            keyTyped('!', Keyboard.KEY_1);
            keyTyped('\b', Keyboard.KEY_BACK);
            // click far away: unfocus
            mouseClicked(2, 2, 0);
        } catch (IOException e) {
            FTGui.T.log("GUI-03", "io-exception", "type", e.getClass().getName());
        }
        FTGui.T.log("GUI-03", "after-input", "text", field.getText(), "focused", field.isFocused(), "lastAction", lastAction);
    }

    private String lastAction = "none";

    @Override
    protected void actionPerformed(GuiButton button) throws IOException {
        lastAction = "button" + button.id;
        FTGui.T.log("GUI-03", "actionPerformed", "id", button.id, "label", button.displayString);
    }

    @Override
    protected void mouseClicked(int mouseX, int mouseY, int mouseButton) throws IOException {
        super.mouseClicked(mouseX, mouseY, mouseButton);
        field.mouseClicked(mouseX, mouseY, mouseButton);
    }

    @Override
    protected void keyTyped(char c, int code) throws IOException {
        if (field.isFocused()) field.textboxKeyTyped(c, code);
        else super.keyTyped(c, code);
    }

    @Override
    public void updateScreen() {
        field.updateCursorCounter();
    }

    @Override
    public void onGuiClosed() {
        FTGui.T.log("GUI-02", "onGuiClosed");
    }

    @Override
    public boolean doesGuiPauseGame() {
        return false;
    }
}
