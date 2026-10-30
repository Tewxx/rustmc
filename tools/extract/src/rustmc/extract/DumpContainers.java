package rustmc.extract;

import com.google.gson.*;
import com.mojang.authlib.GameProfile;
import java.util.UUID;
import net.minecraft.entity.player.EntityPlayer;
import net.minecraft.init.Items;
import net.minecraft.inventory.*;
import net.minecraft.item.Item;
import net.minecraft.item.ItemStack;

/** Traces real Container.slotClick state and return values, including C0E's clicked stack. */
final class DumpContainers {
    static final class Player extends EntityPlayer {
        Player() { super(new DumpMovement.MoveWorld(), new GameProfile(new UUID(0, 1), "fixture")); }
        public boolean isSpectator() { return false; }
    }
    static JsonElement stack(ItemStack s) {
        if (s == null) return JsonNull.INSTANCE;
        JsonArray a = new JsonArray(); a.add(new JsonPrimitive(Item.getIdFromItem(s.getItem())));
        a.add(new JsonPrimitive(s.stackSize)); a.add(new JsonPrimitive(s.getMetadata())); return a;
    }
    static JsonObject run(String name, boolean playerWindow, int[][] initial, int[][] clicks) {
        Player p = new Player();
        Container c = playerWindow ? p.inventoryContainer : new ContainerChest(p.inventory, new InventoryBasic("chest", false, 27), p);
        JsonObject out = new JsonObject(); out.addProperty("name", name); out.addProperty("kind", playerWindow ? "player" : "minecraft:chest");
        JsonArray init = new JsonArray();
        for (int[] x : initial) {
            c.getSlot(x[0]).putStack(new ItemStack(Item.getItemById(x[1]), x[2], x[3]));
            JsonArray row = new JsonArray(); for (int i : x) row.add(new JsonPrimitive(i)); init.add(row);
        }
        out.add("initial", init);
        JsonArray steps = new JsonArray();
        for (int[] click : clicks) {
            ItemStack ret = c.slotClick(click[0], click[1], click[2], p);
            JsonObject step = new JsonObject(); step.addProperty("slot", click[0]); step.addProperty("button", click[1]); step.addProperty("mode", click[2]);
            step.add("returned", stack(ret)); step.add("cursor", stack(p.inventory.getItemStack()));
            JsonArray slots = new JsonArray();
            for (int i = 0; i < c.inventorySlots.size(); i++) slots.add(stack(c.getSlot(i).getStack()));
            step.add("slots", slots); steps.add(step);
        }
        out.add("steps", steps); return out;
    }
    static JsonArray dump() {
        JsonArray a = new JsonArray();
        a.add(run("pickup", false, new int[][] {{0, 1, 41, 0}, {1, 1, 60, 0}, {2, 4, 2, 0}}, new int[][] {{0,1,0},{1,0,0},{2,0,0},{3,1,0},{3,0,0}}));
        a.add(run("shift and number key", false, new int[][] {{0,1,40,0},{54,1,20,0},{55,4,4,0}}, new int[][] {{0,0,1},{55,0,2},{2,1,2},{54,0,1}}));
        int[][] full = new int[37][4]; full[0] = new int[] {0, 1, 2, 0};
        for (int i=0; i<36; i++) full[i+1] = new int[] {27+i, 4, 64, 0};
        a.add(run("number key chest", false, new int[][] {{0, 4, 5, 0}, {54, 1, 7, 0}}, new int[][] {{0, 0, 2}}));
        a.add(run("failed shift", false, full, new int[][] {{0,0,1}}));
        a.add(run("armor slots", true, new int[][] {{36,1,5,0}, {37,306,1,0}}, new int[][] {{36,0,0},{5,0,0},{36,0,0},{37,0,1},{5,0,2}}));
        return a;
    }
}
