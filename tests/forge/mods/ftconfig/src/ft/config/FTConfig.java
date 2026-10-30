package ft.config;

import ft.Trace;
import java.io.File;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.nio.file.Files;
import java.util.*;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.GuiScreen;
import net.minecraftforge.common.config.ConfigCategory;
import net.minecraftforge.common.config.Configuration;
import net.minecraftforge.common.config.Property;
import net.minecraftforge.fml.common.Mod;
import net.minecraftforge.fml.common.event.FMLPreInitializationEvent;
import net.minecraftforge.fml.common.event.FMLInitializationEvent;
import net.minecraftforge.fml.common.ObfuscationReflectionHelper;
import net.minecraftforge.fml.relauncher.ReflectionHelper;

/**
 * CFG-*: Configuration read/write (file text compared byte-for-byte), REFL-*: ReflectionHelper /
 * ObfuscationReflectionHelper on SRG names, AT-*: access transformer (META-INF/ftconfig_at.cfg).
 */
@Mod(modid = "ftconfig", name = "FT Config", version = "1.0", dependencies = "required-after:ftcommon",
     acceptableRemoteVersions = "*", clientSideOnly = true)
public class FTConfig {
    static final Trace.T T = Trace.of("config");

    @Mod.EventHandler
    public void preInit(FMLPreInitializationEvent e) throws Exception {
        File f = e.getSuggestedConfigurationFile();
        f.delete(); // start from a clean file so the written text is deterministic
        Configuration c = new Configuration(f, "1.0");
        c.load();
        int i = c.getInt("count", "general", 5, 0, 10, "How many things");
        boolean b = c.getBoolean("enabled", "general", true, "Enable the thing");
        String s = c.getString("name", "general", "default name", "A name");
        String pick = c.getString("pick", "choices", "b", "One of a, b, c", new String[] {"a", "b", "c"});
        float fl = c.getFloat("ratio", "general", 0.5f, 0f, 1f, "A ratio");
        double d = c.get("general", "precise", 1.25, "A double").getDouble();
        String[] list = c.getStringList("items", "lists", new String[] {"x", "y"}, "A list");
        int[] ints = c.get("lists", "numbers", new int[] {1, 2, 3}, "Ints").getIntList();
        c.addCustomCategoryComment("choices", "Category comment");
        T.log("CFG-01", "defaults", "count", i, "enabled", b, "name", s, "pick", pick, "ratio", fl, "precise", d,
            "items", Arrays.asList(list), "numbers", Arrays.toString(ints), "hasChanged", c.hasChanged(),
            "categories", new TreeSet<String>(c.getCategoryNames()), "hasKey", c.hasKey("general", "count"),
            "hasCategory", c.hasCategory("nope"));
        c.save();
        T.log("CFG-02", "file-after-first-save", "lines", Files.readAllLines(f.toPath(), java.nio.charset.Charset.forName("UTF-8")));

        // out-of-range value is clamped on read, modified values persist across a reload
        Property p = c.get("general", "count", 5);
        p.set(99);
        c.get("general", "name", "x").set("changed");
        c.save();
        Configuration c2 = new Configuration(f, "1.0");
        c2.load();
        T.log("CFG-03", "reload", "countClamped", c2.getInt("count", "general", 5, 0, 10, "How many things"),
            "rawCount", c2.get("general", "count", 5).getInt(), "name", c2.getString("name", "general", "default name", "A name"),
            "propType", c2.get("general", "count", 5).getType(), "isDefault", c2.get("general", "ratio", 0.5).isDefault());
        ConfigCategory cat = c2.getCategory("general");
        List<String> keys = new ArrayList<String>(cat.keySet());
        Collections.sort(keys);
        T.log("CFG-03", "category", "name", cat.getName(), "keys", keys, "comment", cat.getComment());
        T.log("CFG-04", "file-after-second-save", "lines", Files.readAllLines(f.toPath(), java.nio.charset.Charset.forName("UTF-8")));
    }

    @Mod.EventHandler
    public void init(FMLInitializationEvent e) {
        Minecraft mc = Minecraft.getMinecraft();
        // ---- reflection on SRG names
        try {
            Object timer = ObfuscationReflectionHelper.getPrivateValue(Minecraft.class, mc, "timer", "field_71428_T");
            T.log("REFL-01", "getPrivateValue(Object)", "class", timer.getClass().getName());
            int delay = ObfuscationReflectionHelper.getPrivateValue(Minecraft.class, mc, "rightClickDelayTimer", "field_71467_ac");
            ObfuscationReflectionHelper.setPrivateValue(Minecraft.class, mc, 7, "rightClickDelayTimer", "field_71467_ac");
            int after = ReflectionHelper.getPrivateValue(Minecraft.class, mc, "rightClickDelayTimer", "field_71467_ac");
            T.log("REFL-02", "set/getPrivateValue(int)", "before", delay, "after", after);
            ObfuscationReflectionHelper.setPrivateValue(Minecraft.class, mc, delay, "rightClickDelayTimer", "field_71467_ac");
        } catch (Throwable t) {
            T.log("REFL-01", "failed", "type", t.getClass().getName(), "msg", String.valueOf(t.getMessage()));
        }
        try {
            ReflectionHelper.getPrivateValue(Minecraft.class, mc, "noSuchField", "field_000000_x");
            T.log("REFL-03", "bogus-field", "threw", false);
        } catch (Throwable t) {
            T.log("REFL-03", "bogus-field", "threw", t.getClass().getName());
        }
        try {
            Method m = ReflectionHelper.findMethod(GuiScreen.class, null, new String[] {"drawWorldBackground", "func_146270_b"}, int.class);
            T.log("REFL-04", "findMethod", "name", m == null ? null : "found", "modifiers", Modifier.toString(m.getModifiers()));
            Class<?> c = ReflectionHelper.getClass(getClass().getClassLoader(), "net.minecraft.client.gui.GuiScreen");
            T.log("REFL-04", "getClass", "name", c.getName());
        } catch (Throwable t) {
            T.log("REFL-04", "failed", "type", t.getClass().getName());
        }

        // ---- access transformer: ftconfig_at.cfg makes these public at class-load time
        try {
            java.lang.reflect.Field fld = null;
            for (String n : new String[] {"field_71467_ac", "rightClickDelayTimer"}) {
                try { fld = Minecraft.class.getDeclaredField(n); break; } catch (NoSuchFieldException x) { }
            }
            T.log("AT-01", "field-modifiers", "public", Modifier.isPublic(fld.getModifiers()), "final", Modifier.isFinal(fld.getModifiers()));
            Method pub = null;
            for (String n : new String[] {"func_147121_ag", "rightClickMouse"}) {
                try { pub = Minecraft.class.getMethod(n); break; } catch (NoSuchMethodException x) { }   // getMethod: public only
            }
            T.log("AT-02", "method-public", "foundViaGetMethod", pub != null);
            java.lang.reflect.Field timerFld = null;
            for (String n : new String[] {"field_71428_T", "timer"}) {
                try { timerFld = Minecraft.class.getDeclaredField(n); break; } catch (NoSuchFieldException x) { }
            }
            T.log("AT-03", "final-removed", "timerFinal", Modifier.isFinal(timerFld.getModifiers()), "timerPublic", Modifier.isPublic(timerFld.getModifiers()));
        } catch (Throwable t) {
            T.log("AT-01", "failed", "type", t.getClass().getName());
        }
    }
}
