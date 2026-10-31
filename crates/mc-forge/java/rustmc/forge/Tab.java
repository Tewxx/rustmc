package rustmc.forge;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.mojang.authlib.GameProfile;
import java.lang.reflect.Field;
import java.util.HashMap;
import java.util.HashSet;
import java.util.Map;
import java.util.Set;
import java.util.UUID;
import net.minecraft.client.Minecraft;
import net.minecraft.client.multiplayer.WorldClient;

/** The tab list (NetHandlerPlayClient.playerInfoMap) and the world scoreboard's teams, mirrored from Rust. */
public final class Tab {
    private static final Map<Class<?>, Map<String, Field>> FIELDS = new HashMap<>();
    /** The world whose scoreboard the last snapshot mirrored; there is no handle in invoke's signature. */
    private static Object world;

    /** Live.sync calls this once per snapshot with `{players:[{uuid,name,display,ping,game_mode}],
     *  teams:[{name,display,prefix,suffix,color,friendly_flags,visibility,members}]}`. */
    public static void sync(Minecraft mc, WorldClient world, JsonObject tab) throws Exception {
        Tab.world = world;
        Object player = mc == null ? null : get(mc, "field_71439_g");
        Object handler = player == null ? null : get(player, "field_71174_a");
        if (handler != null) players(handler, array(tab, "players"));
        if (world != null) teams(world, array(tab, "teams"));
    }

    /** NetHandlerPlayClient.playerInfoMap, reusing one NetworkPlayerInfo per uuid (mods hold on to them). */
    @SuppressWarnings("unchecked")
    private static void players(Object handler, JsonArray list) throws Exception {
        Map<UUID, Object> map = infoMap(handler);
        Set<UUID> seen = new HashSet<UUID>();
        for (JsonElement element : list) {
            JsonObject p = element.getAsJsonObject();
            UUID uuid = uuid(attr(p, "uuid"));
            String name = attr(p, "name");
            seen.add(uuid);
            Object info = map.get(uuid);
            if (info == null || !info.getClass().getName().equals("net.minecraft.client.network.NetworkPlayerInfo")) {
                info = allocate("net.minecraft.client.network.NetworkPlayerInfo");
                map.put(uuid, info);
            }
            set(info, "field_178867_a", new GameProfile(uuid, name));
            set(info, "field_178866_b", gameType(num(p, "game_mode")));
            set(info, "field_78829_b", num(p, "ping"));
            String display = attr(p, "display");
            set(info, "field_178872_h", display == null || display.isEmpty() ? null : text(display));
        }
        map.keySet().retainAll(seen);
    }

    /** World.worldScoreboard, created on demand, with ScorePlayerTeam mirrors in its two team maps. */
    @SuppressWarnings("unchecked")
    private static void teams(Object world, JsonArray list) throws Exception {
        Object board = get(world, "field_96442_D");
        if (board == null || !board.getClass().getName().equals("net.minecraft.scoreboard.Scoreboard")) {
            board = allocate("net.minecraft.scoreboard.Scoreboard");
            set(world, "field_96442_D", board);
        }
        Map<String, Object> teams = teamMap(board, "field_96542_e");
        Map<String, Object> members = teamMap(board, "field_96540_f");
        members.clear(); // rebuilt below: a player's team changes only through this map
        Set<String> names = new HashSet<String>();
        for (JsonElement element : list) {
            JsonObject t = element.getAsJsonObject();
            String name = attr(t, "name");
            if (name == null) continue;
            names.add(name);
            Object team = teams.get(name);
            if (team == null || !team.getClass().getName().equals("net.minecraft.scoreboard.ScorePlayerTeam")) {
                team = allocate("net.minecraft.scoreboard.ScorePlayerTeam");
                teams.put(name, team);
            }
            String display = attr(t, "display");
            set(team, "field_96677_a", board);
            set(team, "field_96675_b", name);
            set(team, "field_96673_d", display == null || display.isEmpty() ? name : display);
            set(team, "field_96674_e", blank(attr(t, "prefix")));
            set(team, "field_96671_f", blank(attr(t, "suffix")));
            int flags = num(t, "friendly_flags");
            set(team, "field_96672_g", (flags & 1) != 0);
            set(team, "field_98301_h", (flags & 2) != 0);
            Object visibility = visible(attr(t, "visibility"));
            set(team, "field_178778_i", visibility);
            set(team, "field_178776_j", visibility);
            set(team, "field_178777_k", format(num(t, "color")));
            Set<String> set = (Set<String>) get(team, "field_96676_c");
            if (set == null) { set = new HashSet<String>(); set(team, "field_96676_c", set); }
            set.clear();
            JsonElement listed = t.get("members");
            if (listed != null && listed.isJsonArray()) for (JsonElement m : listed.getAsJsonArray()) {
                String member = m.getAsString();
                set.add(member);
                members.put(member, team);
            }
        }
        teams.keySet().retainAll(names);
    }

    /** Rust -> Java method dispatch, keyed by the SRG `class.method` of the stub that was hit. */
    public static Object invoke(String key, Object self, Object[] args) throws Exception {
        String method = key.substring(key.lastIndexOf('.') + 1);
        if (method.equals("func_147114_u")) { // Minecraft.getNetHandler
            Object player = get(self, "field_71439_g");
            return player == null ? null : get(player, "field_71174_a");
        }
        if (method.equals("func_175106_d")) return infoMap(self).values(); // getPlayerInfoMap
        if (method.equals("func_175102_a")) return infoMap(self).get((UUID) args[0]); // getPlayerInfo(UUID)
        if (method.equals("func_175104_a")) { // getPlayerInfo(String)
            String name = (String) args[0];
            for (Object info : infoMap(self).values()) {
                String own = profileName(info);
                if (own != null && own.equals(name)) return info;
            }
            return null;
        }
        if (method.equals("func_178845_a")) return get(self, "field_178867_a"); // getGameProfile
        if (method.equals("func_178848_b")) return get(self, "field_178866_b"); // getGameType
        if (method.equals("func_178853_c")) return get(self, "field_78829_b"); // getResponseTime
        if (method.equals("func_178854_k")) return get(self, "field_178872_h"); // getDisplayName
        if (method.equals("func_178850_i")) return playersTeam(board(), profileName(self)); // getPlayerTeam
        if (method.equals("func_96441_U")) return get(self, "field_96442_D"); // World.getScoreboard
        if (method.equals("func_96508_e")) return teams(self).get((String) args[0]); // getTeam(String)
        if (method.equals("func_96509_i")) return teamMap(self, "field_96540_f").get((String) args[0]); // getPlayersTeam
        if (method.equals("func_96525_g")) return teams(self).values(); // getTeams
        if (method.equals("func_96531_f")) return teams(self).keySet(); // getTeamNames
        if (method.equals("func_96661_b")) return get(self, "field_96675_b"); // getRegisteredName
        if (method.equals("func_96669_c")) return get(self, "field_96673_d"); // getTeamName
        if (method.equals("func_96668_e")) return colorPrefix(self); // getColorPrefix
        if (method.equals("func_96663_f")) return colorSuffix(self); // getColorSuffix
        if (method.equals("func_96670_d")) return get(self, "field_96676_c"); // getMembershipCollection
        if (method.equals("func_178775_l")) return get(self, "field_178777_k"); // getChatFormat
        if (method.equals("func_142053_d")) return colorPrefix(self) + args[0] + colorSuffix(self); // formatString
        if (method.equals("func_96667_a")) // static Team.formatPlayerName(Team, String)
            return args[0] == null ? args[1] : colorPrefix(args[0]) + args[1] + colorSuffix(args[0]);
        if (method.equals("func_142054_a")) return self == args[0]; // Team.isSameTeam: same as ==
        if (method.equals("func_96124_cp")) return teamOf(self); // EntityLivingBase/EntityPlayer.getTeam
        if (method.equals("func_142014_c")) { // isOnSameTeam: own team set and identical to the other's
            Object own = teamOf(self);
            return own != null && own == teamOf(args[0]);
        }
        return null;
    }

    // ---- scoreboard lookups ------------------------------------------------------------------

    private static Object board() throws Exception {
        Object w = world;
        if (w == null) {
            Minecraft mc = Minecraft.func_71410_x();
            w = mc == null ? null : get(mc, "field_71441_e");
        }
        return w == null ? null : get(w, "field_96442_D");
    }

    private static Object playersTeam(Object board, String name) throws Exception {
        return board == null || name == null ? null : teamMap(board, "field_96540_f").get(name);
    }

    private static Object teamOf(Object entity) throws Exception {
        Object w = entity == null ? null : get(entity, "field_70170_p");
        return w == null ? null : playersTeam(get(w, "field_96442_D"), nameOf(entity));
    }

    /** EntityPlayer's profile name; other entities are keyed by Live's synthetic uuid, so they match nothing. */
    private static String nameOf(Object entity) throws Exception {
        try {
            GameProfile profile = (GameProfile) get(entity, "field_146106_i");
            if (profile != null) return profile.getName();
        } catch (NoSuchFieldException notAPlayer) { }
        return new UUID(0L, ((Number) get(entity, "field_145783_c")).longValue()).toString();
    }

    private static String profileName(Object info) throws Exception {
        GameProfile profile = (GameProfile) get(info, "field_178867_a");
        return profile == null ? null : profile.getName();
    }

    /** ScorePlayerTeam.getColorPrefix: vanilla returns the prefix field as is. */
    private static String colorPrefix(Object team) throws Exception {
        return (String) get(team, "field_96674_e");
    }

    /** ScorePlayerTeam.getColorSuffix: the suffix field as is. */
    private static String colorSuffix(Object team) throws Exception {
        return (String) get(team, "field_96671_f");
    }

    private static Object gameType(int mode) throws Exception {
        Object[] all = constants("net.minecraft.world.WorldSettings$GameType");
        int index = mode + 1; // NOT_SET(-1) first, then SURVIVAL(0), CREATIVE(1), ADVENTURE(2), SPECTATOR(3)
        return all == null || all.length == 0 || index < 0 || index >= all.length ? null : all[index];
    }

    private static Object visible(String name) throws Exception {
        Object[] all = constants("net.minecraft.scoreboard.Team$EnumVisible");
        if (all == null || all.length == 0) return null;
        for (Object value : all) if (name != null && name.equalsIgnoreCase((String) get(value, "field_178830_e"))) return value;
        return all[0]; // ALWAYS
    }

    private static Object format(int color) throws Exception {
        Object[] all = constants("net.minecraft.util.EnumChatFormatting");
        if (all == null || all.length == 0) return null;
        for (Object value : all) if (Integer.valueOf(color).equals(get(value, "field_175747_C"))) return value;
        return all[all.length - 1]; // RESET
    }

    private static Object[] constants(String name) throws Exception {
        Class<?> type = Class.forName(name, true, Tab.class.getClassLoader());
        return type.getEnumConstants();
    }

    /** ChatComponentText is real vanilla code (keep-real.txt), so its constructor builds the sibling list and style. */
    private static Object text(String value) { return new net.minecraft.util.ChatComponentText(value); }

    // ---- reflection helpers (Live's are private) ---------------------------------------------

    @SuppressWarnings("unchecked")
    private static <T> T allocate(Class<T> type) { return (T) Stub.call("rustmc.allocate", null, new Object[] {type}); }

    private static Object allocate(String name) throws Exception {
        return allocate(Class.forName(name, true, Tab.class.getClassLoader()));
    }

    private static Field field(Class<?> type, String name) throws NoSuchFieldException {
        Map<String, Field> fields = FIELDS.get(type);
        if (fields == null) { fields = new HashMap<String, Field>(); FIELDS.put(type, fields); }
        Field found = fields.get(name);
        if (found == null) {
            for (Class<?> c = type; c != null && found == null; c = c.getSuperclass()) {
                try { found = c.getDeclaredField(name); } catch (NoSuchFieldException ignored) { }
            }
            if (found == null) throw new NoSuchFieldException(type.getName() + "." + name);
            found.setAccessible(true);
            fields.put(name, found);
        }
        return found;
    }

    private static Object get(Object object, String name) throws Exception { return field(object.getClass(), name).get(object); }

    private static void set(Object object, String name, Object value) throws Exception { field(object.getClass(), name).set(object, value); }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> teams(Object board) throws Exception { return teamMap(board, "field_96542_e"); }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> teamMap(Object board, String name) throws Exception {
        Map<String, Object> map = (Map<String, Object>) get(board, name);
        if (map == null) { map = new HashMap<String, Object>(); set(board, name, map); }
        return map;
    }

    @SuppressWarnings("unchecked")
    private static Map<UUID, Object> infoMap(Object handler) throws Exception {
        Map<UUID, Object> map = (Map<UUID, Object>) get(handler, "field_147310_i");
        if (map == null) { map = new HashMap<UUID, Object>(); set(handler, "field_147310_i", map); }
        return map;
    }

    // ---- json helpers -------------------------------------------------------------------------

    private static JsonArray array(JsonObject owner, String name) {
        JsonElement element = owner == null ? null : owner.get(name);
        return element == null || !element.isJsonArray() ? new JsonArray() : element.getAsJsonArray();
    }

    private static String attr(JsonObject owner, String name) {
        JsonElement element = owner.get(name);
        return element == null || element.isJsonNull() ? null : element.getAsString();
    }

    private static int num(JsonObject owner, String name) {
        JsonElement element = owner.get(name);
        return element == null || element.isJsonNull() ? 0 : element.getAsInt();
    }

    private static String blank(String value) { return value == null ? "" : value; }

    private static UUID uuid(String value) {
        if (value != null) {
            if (value.length() == 32) value = value.substring(0, 8) + "-" + value.substring(8, 12) + "-" + value.substring(12, 16)
                + "-" + value.substring(16, 20) + "-" + value.substring(20);
            try { return UUID.fromString(value); } catch (IllegalArgumentException ignored) { }
        }
        return new UUID(0L, value == null ? 0L : value.hashCode());
    }
}
