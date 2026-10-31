// Vanilla 1.8.9 reference renderer: boots the real client (compiled MCP classes), loads/creates a
// singleplayer world, puts the camera at a fixed pose and saves screenshots through ScreenShotHelper.
// Usage: VisualHarness <gameDir> <scenes.txt> <width> <height> [only-scene-substring]
// Scene line: name world x y z yaw pitch time [gui]   (x y z = EYE position; y "auto" = surface + 1.62)
// Writes <gameDir>/screenshots/<name>.png and <gameDir>/screenshots/<name>.cam.
import net.minecraft.block.Block;
import net.minecraft.block.state.IBlockState;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.*;
import net.minecraft.client.gui.inventory.GuiInventory;
import net.minecraft.client.multiplayer.GuiConnecting;
import net.minecraft.client.multiplayer.ServerData;
import net.minecraft.client.renderer.RenderGlobal;
import net.minecraft.entity.player.EntityPlayerMP;
import net.minecraft.init.Blocks;
import net.minecraft.server.MinecraftServer;
import net.minecraft.util.BlockPos;
import net.minecraft.util.ChatComponentText;
import net.minecraft.util.ScreenShotHelper;
import net.minecraft.world.EnumDifficulty;
import net.minecraft.world.WorldServer;
import net.minecraft.world.WorldSettings;
import net.minecraft.world.WorldType;
import org.lwjgl.input.Mouse;

import java.io.File;
import java.io.PrintWriter;
import java.lang.reflect.Field;
import java.nio.file.Files;
import java.util.*;
import java.util.concurrent.Callable;

public class VisualHarness {
    static Minecraft mc;
    static File gameDir;
    static int W, H;

    public static void main(String[] a) throws Exception {
        gameDir = new File(a[0]).getAbsoluteFile();
        File scenes = new File(a[1]);
        W = Integer.parseInt(a[2]);
        H = Integer.parseInt(a[3]);
        String only = a.length > 4 ? a[4] : "";
        gameDir.mkdirs();
        Thread driver = new Thread(() -> {
            try {
                drive(scenes, only);
            } catch (Throwable t) {
                t.printStackTrace();
                System.exit(2);
            }
            System.exit(0);
        }, "driver");
        driver.setDaemon(true);
        driver.start();
        net.minecraft.client.main.Main.main(new String[] {
            "--gameDir", gameDir.getPath(), "--assetsDir", System.getProperty("harness.assets"),
            "--assetIndex", "1.8", "--version", "1.8.9", "--accessToken", "0", "--username", "Player",
            "--width", "" + W, "--height", "" + H});
    }

    static void sleep(long ms) {
        try { Thread.sleep(ms); } catch (InterruptedException e) { throw new RuntimeException(e); }
    }

    static void client(Runnable r) throws Exception { mc.addScheduledTask(r).get(); }

    static Object field(Object o, String name) throws Exception {
        Field f = o.getClass().getDeclaredField(name);
        f.setAccessible(true);
        return f.get(o);
    }

    static void drive(File scenes, String only) throws Exception {
        while (mc == null || mc.currentScreen == null || mc.getResourceManager() == null || mc.theWorld != null) {
            mc = Minecraft.getMinecraft();
            sleep(200);
        }
        sleep(3000);
        client(() -> {
            // Fixed settings, mirrored by `rustmc --screenshot` (see tools/visual/run.sh).
            mc.gameSettings.renderDistanceChunks = 16;
            mc.gameSettings.fancyGraphics = true;
            mc.gameSettings.ambientOcclusion = 2;
            mc.gameSettings.guiScale = 2;
            mc.gameSettings.fovSetting = 70f;
            mc.gameSettings.viewBobbing = false;
            mc.gameSettings.hideGUI = true;
            mc.gameSettings.clouds = 0;
            mc.gameSettings.gammaSetting = 0f;
            mc.gameSettings.enableVsync = false;
            mc.gameSettings.limitFramerate = 260;
            mc.gameSettings.showDebugInfo = false;
            mc.gameSettings.pauseOnLostFocus = false;
            mc.gameSettings.difficulty = EnumDifficulty.PEACEFUL;
            mc.gameSettings.snooperEnabled = false;
        });
        File shots = new File(gameDir, "screenshots");
        shots.mkdirs();
        // group scenes by world, keep order
        LinkedHashMap<String, List<String[]>> byWorld = new LinkedHashMap<>();
        for (String line : Files.readAllLines(scenes.toPath())) {
            line = line.trim();
            if (line.isEmpty() || line.startsWith("#")) continue;
            String[] p = line.split("\\s+");
            if (!only.isEmpty() && !p[0].contains(only)) continue;
            byWorld.computeIfAbsent(p[1], k -> new ArrayList<>()).add(p);
        }
        String only_world = System.getProperty("harness.world", "");
        if (only_world.isEmpty() || only_world.equals("title"))
            for (String[] sc : byWorld.getOrDefault("title", Collections.emptyList())) titleScene(sc, shots);
        byWorld.remove("title");
        for (Map.Entry<String, List<String[]>> e : byWorld.entrySet()) {
            String world = e.getKey();
            if (!only_world.isEmpty() && !only_world.equals(world)) continue;
            connect();
            setup(world);
            for (String[] sc : e.getValue()) scene(sc, shots);
        }
        sleep(500);
        Runtime.getRuntime().halt(0);
    }

    static void cmd(String c) throws Exception {
        client(() -> mc.thePlayer.sendChatMessage(c));
        sleep(150);
    }

    /** Join the vanilla server (offline mode, `Player` is op) like the multiplayer screen would. */
    static void connect() throws Exception {
        int port = Integer.getInteger("harness.port", 25871);
        client(() -> mc.displayGuiScreen(new GuiConnecting(new GuiMainMenu(), mc, new ServerData("visual", "127.0.0.1:" + port, false))));
        for (int i = 0; i < 600 && (mc.thePlayer == null || mc.theWorld == null); i++) sleep(100);
        if (mc.thePlayer == null) throw new RuntimeException("could not join the server");
        sleep(4000);
    }

    static void setup(String world) throws Exception {
        for (String c : new String[] {"/gamemode 1", "/difficulty 0", "/weather clear 1000000", "/gamerule doDaylightCycle false",
                "/gamerule doMobSpawning false", "/gamerule randomTickSpeed 0", "/gamerule doFireTick false", "/gamerule mobGriefing false"})
            cmd(c);
        if (world.equals("grid")) buildGrid();
    }

    static final Map<Integer, String> NAMES = new HashMap<>();
    static {
        String[] n = {"0 air","1 stone","2 grass","3 dirt","4 cobblestone","5 planks","6 sapling","9 water","11 lava","12 sand","17 log","18 leaves",
            "20 glass","22 lapis_block","24 sandstone","31 tallgrass","35 wool","37 yellow_flower","38 red_flower","41 gold_block","44 stone_slab",
            "45 brick_block","49 obsidian","50 torch","53 oak_stairs","54 chest","58 crafting_table","61 furnace","73 redstone_ore","78 snow_layer",
            "79 ice","81 cactus","83 reeds","85 fence","87 netherrack","89 glowstone","95 stained_glass","102 glass_pane","113 nether_brick_fence",
            "130 ender_chest","139 cobblestone_wall","175 double_plant"};
        for (String e : n) { String[] kv = e.split(" "); NAMES.put(Integer.parseInt(kv[0]), "minecraft:" + kv[1]); }
    }
    static void block(int x, int y, int z, int id, int meta) throws Exception { cmd("/setblock " + x + " " + y + " " + z + " " + NAMES.get(id) + " " + meta); }
    static void fill(int x0, int y0, int z0, int x1, int y1, int z1, int id) throws Exception {
        cmd("/fill " + x0 + " " + y0 + " " + z0 + " " + x1 + " " + y1 + " " + z1 + " " + NAMES.get(id));
    }

    /** Flat stone world (floor top y=60); the player stands on y=61. Mirrors tools/visual/scenes.txt. */
    static void buildGrid() throws Exception {
        cmd("/tp Player 0 70 10");
        sleep(6000);
        fill(-30, 61, -20, 30, 68, 40, 0);
        int[][][] rows = {
            {{1,0},{5,0},{4,0},{17,0},{89,0},{45,0},{41,0},{24,0},{87,0},{35,14}},
            {{44,0},{44,8},{53,0},{53,4},{85,0},{113,0},{139,0},{20,0},{102,0},{95,14}},
            {{2,0},{18,0},{18,1},{31,1},{38,0},{37,0},{50,5},{6,0},{83,0},{175,2}},
            {{54,2},{130,2},{61,2},{58,0},{79,0},{78,0},{81,0},{22,0},{73,0},{49,0}},
        };
        for (int r = 0; r < rows.length; r++) {
            for (int i = 0; i < rows[r].length; i++) {
                int x = -9 + 2 * i, z = 3 * r, id = rows[r][i][0], meta = rows[r][i][1];
                if (id == 31 || id == 38 || id == 37 || id == 6 || id == 83 || id == 175) block(x, 60, z, 2, 0);
                if (id == 81) block(x, 60, z, 12, 0);
                block(x, 61, z, id, meta);
                if (id == 175) block(x, 62, z, 175, 10);
            }
        }
        fill(-12, 60, 13, -10, 60, 15, 9);
        fill(-6, 60, 13, -4, 60, 15, 11);
        fill(4, 61, 12, 8, 64, 16, 20);
        fill(5, 61, 13, 7, 63, 15, 0);
        block(6, 61, 14, 50, 5);
        fill(-3, 61, 20, 3, 64, 20, 1);
        fill(3, 61, 17, 3, 63, 19, 1);
        // cave under x 40..58: stepped boxes, a torch, glowstone, lava, water and a shaft up to the sky
        fill(42, 41, -6, 54, 46, 6, 0);
        fill(50, 41, 2, 58, 45, 8, 0);
        fill(44, 41, -8, 50, 47, -2, 0);
        fill(48, 47, 0, 49, 60, 1, 0);
        block(46, 41, 1, 50, 5);
        block(48, 40, -3, 89, 0);
        block(53, 40, 4, 11, 0);
        block(44, 40, 3, 9, 0);
        sleep(1000);
    }

    // Scene line: name title 0 0 0 0 0 0 <screen>   (screen: main | options)
    static void titleScene(String[] p, File shots) throws Exception {
        String name = p[0];
        sleep(2000);
        client(() -> {
            mc.gameSettings.hideGUI = false;
            GuiScreen g = p.length > 8 && p[8].equals("options") ? new GuiOptions(new GuiMainMenu(), mc.gameSettings) : new GuiMainMenu();
            mc.displayGuiScreen(g);
            Mouse.setCursorPosition(W / 2, H - (H / 4 + 48 + 5) * 2 - 10);
        });
        sleep(1500);
        client(() -> {
            try {
                if (mc.currentScreen instanceof GuiMainMenu) {
                    Field f = GuiMainMenu.class.getDeclaredField("splashText");
                    f.setAccessible(true);
                    f.set(mc.currentScreen, "Hello world!");
                    Field t = GuiMainMenu.class.getDeclaredField("panoramaTimer");
                    t.setAccessible(true);
                    t.setInt(mc.currentScreen, 0);
                }
            } catch (Exception e) { throw new RuntimeException(e); }
        });
        client(() -> ScreenShotHelper.saveScreenshot(gameDir, name + ".png", mc.displayWidth, mc.displayHeight, mc.getFramebuffer()));
        try (PrintWriter pw = new PrintWriter(new File(shots, name + ".cam"))) { pw.println("0 0 0 0 0 0"); }
        System.out.println("SHOT " + name);
    }

    static void scene(String[] p, File shots) throws Exception {
        String name = p[0];
        double x = Double.parseDouble(p[2]);
        double z = Double.parseDouble(p[4]);
        float yaw = Float.parseFloat(p[5]), pitch = Float.parseFloat(p[6]);
        long time = Long.parseLong(p[7]);
        String gui = p.length > 8 ? p[8] : "";
        double eyeY;
        if (p[3].equals("auto")) {
            cmd("/tp Player " + x + " 200 " + z);
            sleep(8000);
            eyeY = mc.theWorld.getTopSolidOrLiquidBlock(new BlockPos((int) Math.floor(x), 0, (int) Math.floor(z))).getY() + 1.62;
        } else eyeY = Double.parseDouble(p[3]);
        final double ey = eyeY;
        client(() -> {
            mc.displayGuiScreen(null);
            mc.inGameHasFocus = false; // no mouse-look from the real cursor
            Mouse.setGrabbed(false);
            mc.thePlayer.capabilities.allowFlying = true;
            mc.thePlayer.capabilities.isFlying = true;
            mc.thePlayer.sendPlayerAbilities();
        });
        cmd("/time set " + time);
        cmd("/tp Player " + x + " " + (ey - 1.62) + " " + z + " " + yaw + " " + pitch);
        // Wait for chunks and mesh updates to settle.
        sleep(5000);
        RenderGlobal rg = mc.renderGlobal;
        int quiet = 0;
        for (int i = 0; i < 1200 && quiet < 30; i++) {
            Set<?> pending = (Set<?>) field(rg, "chunksToUpdate");
            final String[] infoBox = new String[1];
            client(() -> infoBox[0] = rg.getDebugInfoRenders());
            String info = infoBox[0];
            boolean idle = info.contains("pC: 000, pU: 0");
            quiet = pending.isEmpty() && idle ? quiet + 1 : 0;
            sleep(100);
        }
        if (quiet < 30) throw new RuntimeException("chunk meshes did not settle for " + name);
        sleep(1500);
        client(() -> mc.guiAchievement.clearAchievements());
        sleep(300);
        if (!gui.isEmpty()) {
            client(() -> {
                mc.gameSettings.hideGUI = false;
                mc.thePlayer.rotationYaw = yaw;
                switch (gui) {
                    case "inventory": mc.displayGuiScreen(new GuiInventory(mc.thePlayer)); break;
                    case "chat":
                        mc.ingameGUI.getChatGUI().printChatMessage(new ChatComponentText("<Player> hello from rustmc"));
                        mc.ingameGUI.getChatGUI().printChatMessage(new ChatComponentText("\u00a7eWelcome! \u00a7aColors \u00a7lbold\u00a7r and \u00a7nunderline"));
                        mc.displayGuiScreen(new GuiChat("typing a message"));
                        break;
                    case "hud": break;
                    case "debug": mc.gameSettings.showDebugInfo = true; break;
                    default: throw new RuntimeException("gui " + gui);
                }
                Mouse.setCursorPosition(W / 2, H / 2);
            });
            sleep(800);
        }
        // Capture the pose on the same client thread as the screenshot, including any server corrections.
        final double[] pose = new double[5];
        client(() -> {
            pose[0] = mc.thePlayer.posX;
            pose[1] = mc.thePlayer.posY + mc.thePlayer.getEyeHeight();
            pose[2] = mc.thePlayer.posZ;
            pose[3] = mc.thePlayer.rotationYaw;
            pose[4] = mc.thePlayer.rotationPitch;
            ScreenShotHelper.saveScreenshot(gameDir, name + ".png", mc.displayWidth, mc.displayHeight, mc.getFramebuffer());
        });
        client(() -> { mc.gameSettings.hideGUI = true; mc.gameSettings.showDebugInfo = false; });
        try (PrintWriter pw = new PrintWriter(new File(shots, name + ".cam"))) {
            pw.println(pose[0] + " " + pose[1] + " " + pose[2] + " " + pose[3] + " " + pose[4] + " " + time);
        }
        System.out.println("SHOT " + name + " " + mc.displayWidth + "x" + mc.displayHeight + String.format(" actual feet %.3f %.3f %.3f yaw %.2f pitch %.2f flying %b", mc.thePlayer.posX, mc.thePlayer.posY, mc.thePlayer.posZ, mc.thePlayer.rotationYaw, mc.thePlayer.rotationPitch, mc.thePlayer.capabilities.isFlying));
    }
}
