package rustmc.extract;

import com.google.gson.*;
import com.mojang.authlib.GameProfile;
import java.util.UUID;
import net.minecraft.entity.player.EntityPlayer;
import net.minecraft.init.Items;
import net.minecraft.inventory.*;
import net.minecraft.item.Item;
import net.minecraft.item.ItemStack;
import net.minecraft.item.crafting.FurnaceRecipes;
import net.minecraft.tileentity.TileEntityFurnace;
import java.util.*;

/** Traces real Container.slotClick state and return values, including C0E's clicked stack. */
final class DumpContainers {
    static final class Player extends EntityPlayer {
        Player() { super(new DumpMovement.MoveWorld(), new GameProfile(new UUID(0, 1), "fixture")); }
        public boolean isSpectator() { return false; }
        public net.minecraft.entity.item.EntityItem dropPlayerItemWithRandomChoice(ItemStack stack, boolean unused) { return null; }
    }
    static JsonElement stack(ItemStack s) {
        if (s == null) return JsonNull.INSTANCE;
        JsonArray a = new JsonArray(); a.add(new JsonPrimitive(Item.getIdFromItem(s.getItem())));
        a.add(new JsonPrimitive(s.stackSize)); a.add(new JsonPrimitive(s.getMetadata())); return a;
    }
    static JsonObject run(String name, String kind, int[][] initial, int[][] clicks) {
        Player p = new Player();
        p.capabilities.isCreativeMode = name.startsWith("creative");
        Container c = kind.equals("player") ? p.inventoryContainer : kind.equals("minecraft:furnace") ? new ContainerFurnace(p.inventory, new TileEntityFurnace()) : new ContainerChest(p.inventory, new InventoryBasic("chest", false, 27), p);
        JsonObject out = new JsonObject(); out.addProperty("name", name); out.addProperty("kind", kind); out.addProperty("creative", p.capabilities.isCreativeMode);
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
        a.add(run("pickup", "minecraft:chest", new int[][] {{0, 1, 41, 0}, {1, 1, 60, 0}, {2, 4, 2, 0}}, new int[][] {{0,1,0},{1,0,0},{2,0,0},{3,1,0},{3,0,0}}));
        a.add(run("shift and number key", "minecraft:chest", new int[][] {{0,1,40,0},{54,1,20,0},{55,4,4,0}}, new int[][] {{0,0,1},{55,0,2},{2,1,2},{54,0,1}}));
        int[][] full = new int[37][4]; full[0] = new int[] {0, 1, 2, 0};
        for (int i=0; i<36; i++) full[i+1] = new int[] {27+i, 4, 64, 0};
        a.add(run("number key chest", "minecraft:chest", new int[][] {{0, 4, 5, 0}, {54, 1, 7, 0}}, new int[][] {{0, 0, 2}}));
        a.add(run("failed shift", "minecraft:chest", full, new int[][] {{0,0,1}}));
        a.add(run("armor slots", "player", new int[][] {{36,1,5,0}, {37,306,1,0}}, new int[][] {{36,0,0},{5,0,0},{36,0,0},{37,0,1},{5,0,2}}));
        a.add(run("shift double", "minecraft:chest", new int[][] {{0,1,10,0},{1,1,5,0},{2,4,4,0},{3,1,2,0}}, new int[][] {{0,0,1},{0,0,1}}));
        a.add(run("creative clone", "minecraft:chest", new int[][] {{0,1,6,0},{1,306,1,0}}, new int[][] {{0,2,3},{1,2,3},{0,0,0},{1,2,3}}));
        a.add(run("drag evenly", "minecraft:chest", new int[][] {{0,1,11,0},{1,1,60,0}}, new int[][] {{0,0,0},{-999,0,5},{1,1,5},{2,1,5},{3,1,5},{-999,2,5}}));
        a.add(run("drag right", "minecraft:chest", new int[][] {{0,1,4,0}}, new int[][] {{0,0,0},{-999,4,5},{1,5,5},{2,5,5},{-999,6,5}}));
        a.add(run("drop items", "minecraft:chest", new int[][] {{0,1,4,0},{1,4,3,0}}, new int[][] {{0,0,4},{1,1,4},{0,0,0},{0,0,4}}));
        a.add(run("collect double click", "minecraft:chest", new int[][] {{0,1,8,0},{1,1,64,0},{2,1,32,0},{3,1,2,0}}, new int[][] {{0,0,0},{0,0,6}}));
        a.add(run("collect reverse", "minecraft:chest", new int[][] {{0,1,20,0},{1,1,32,0},{2,1,10,0}}, new int[][] {{0,1,0},{3,1,6}}));
        a.add(run("furnace routing", "minecraft:furnace", new int[][] {{30,15,12,0},{31,263,8,0},{32,1,5,0}}, new int[][] {{30,0,1},{31,0,1},{32,0,1},{0,0,1},{1,0,1}}));
        a.add(run("furnace fuel validation", "minecraft:furnace", new int[][] {{30,1,8,0},{31,325,1,0},{32,263,4,0}}, new int[][] {{30,0,0},{1,0,0},{30,0,0},{31,0,0},{1,0,0},{1,0,0},{31,0,0},{32,0,0},{1,0,0}}));
        return a;
    }
    static JsonObject furnaceRules() {
        JsonObject root = new JsonObject(); JsonArray fuels = new JsonArray(), smelting = new JsonArray();
        for (int id = 0; id < 4096; id++) {
            Item item = Item.getItemById(id);
            if (item != null && TileEntityFurnace.isItemFuel(new ItemStack(item, 1, 0))) fuels.add(new JsonPrimitive(id));
        }
        List<String> recipes = new ArrayList<String>();
        for (ItemStack input : FurnaceRecipes.instance().getSmeltingList().keySet())
            recipes.add(String.format("%05d:%05d", Item.getIdFromItem(input.getItem()), input.getMetadata()));
        Collections.sort(recipes);
        for (String recipe : recipes) {
            String[] parts = recipe.split(":"); JsonArray row = new JsonArray();
            row.add(new JsonPrimitive(Integer.parseInt(parts[0]))); row.add(new JsonPrimitive(Integer.parseInt(parts[1]))); smelting.add(row);
        }
        root.add("fuels", fuels); root.add("smelting", smelting); return root;
    }
}
