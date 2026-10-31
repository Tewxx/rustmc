package rustmc.forge;

import java.io.File;
import java.text.SimpleDateFormat;
import java.util.Date;
import net.minecraft.client.resources.I18n;
import net.minecraft.util.ChatComponentText;
import net.minecraft.util.ChatComponentTranslation;
import net.minecraft.util.ChatStyle;
import net.minecraft.event.ClickEvent;
import net.minecraft.util.IChatComponent;
import org.lwjgl.input.Keyboard;

/** Glue for client stubs whose logic is plain Java over mirrored state (key names, screenshot files). Anything the
 *  Rust client must do is queued with {@link #action} and drained by impls::client::take_client_actions. */
public final class ClientGlue {
    private ClientGlue() {}

    /** Queues one effect for the Rust client; the values must be String, Number or Boolean. */
    static void action(String kind, Object... values) {
        Stub.call("rustmc/forge/ClientGlue.action", null, new Object[] { kind, values });
    }

    /** `GameSettings.func_74298_c`: vanilla's key name, including its mouse-button form. */
    public static String keyDisplay(int key) {
        if (key < 0) {
            String mouse = I18n.func_135052_a("key.mouseButton", Integer.valueOf(key + 101));
            return mouse != null ? mouse : "Button" + (key + 101);
        }
        if (key < 256) return Keyboard.getKeyName(key);
        return String.format("%c", Character.valueOf((char)(key - 256))).toUpperCase();
    }

    /** `ScreenShotHelper.func_148259_a`: picks the file names the client writes, returns vanilla's chat message. */
    public static IChatComponent screenshot(File gameDirectory, String name, int width, int height) {
        try {
            File dir = new File(gameDirectory == null ? new File(".") : gameDirectory, "screenshots");
            File file = name != null ? new File(dir, name) : timestamped(dir);
            ClientGlue.action("screenshot", file.getAbsolutePath(), Integer.valueOf(width), Integer.valueOf(height));
            IChatComponent label = new ChatComponentText(file.getName());
            ChatStyle style = new ChatStyle();
            // The stub jar hides nested types from source, so ClickEvent.Action is reached by name.
            Class<?> action = Class.forName("net.minecraft.event.ClickEvent$Action", true, ClientGlue.class.getClassLoader());
            @SuppressWarnings({"unchecked", "rawtypes"}) Object open = Enum.valueOf((Class) action, "OPEN_FILE");
            style.func_150241_a(ClickEvent.class.getConstructor(action, String.class).newInstance(open, file.getAbsolutePath()));
            label.func_150255_a(style);
            return new ChatComponentTranslation("screenshot.success", new Object[] { label });
        } catch (Throwable t) {
            return new ChatComponentTranslation("screenshot.failure", new Object[] { String.valueOf(t.getMessage()) });
        }
    }

    /** Vanilla `getTimestampedPNGFileForDirectory`: the first free `yyyy-MM-dd_HH.mm.ss[_N].png`. */
    private static File timestamped(File dir) {
        String stamp = new SimpleDateFormat("yyyy-MM-dd_HH.mm.ss").format(new Date());
        for (int i = 1;; ++i) {
            File file = new File(dir, stamp + (i == 1 ? "" : "_" + i) + ".png");
            if (!file.exists()) return file;
        }
    }

    /** `Scoreboard.func_96539_a`: the objective shown in a display slot (0 tab, 1 sidebar, 2 below name). */
    public static Object objectiveInDisplaySlot(Object board, int slot) throws Exception {
        Object[] slots = (Object[]) field(board, "field_96541_d");
        return slots == null || slot < 0 || slot >= slots.length ? null : slots[slot];
    }

    /** `Scoreboard.func_96528_e`: every tracked entity's Score, flattened. */
    public static Object scores(Object board) throws Exception {
        java.util.List<Object> all = new java.util.ArrayList<Object>();
        java.util.Map<?, ?> byEntity = (java.util.Map<?, ?>) field(board, "field_96544_c");
        if (byEntity != null) for (Object scores : byEntity.values())
            if (scores != null) all.addAll(((java.util.Map<?, ?>) scores).values());
        return all;
    }

    /** A stub field by name (fields keep their SRG name but their visibility from vanilla). */
    private static Object field(Object self, String name) throws Exception {
        for (Class<?> c = self.getClass(); c != null; c = c.getSuperclass()) {
            try {
                java.lang.reflect.Field f = c.getDeclaredField(name);
                f.setAccessible(true);
                return f.get(self);
            } catch (NoSuchFieldException ignored) { }
        }
        return null;
    }
}
