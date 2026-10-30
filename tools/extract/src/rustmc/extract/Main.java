package rustmc.extract;

import com.google.gson.GsonBuilder;
import com.google.gson.JsonElement;
import java.io.File;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.Writer;
import net.minecraft.init.Bootstrap;

/**
 * Golden-data extractor. Runs against the MCP-compiled 1.8.9 classes and writes JSON dumps that the
 * Rust tests compare against. Usage: Main <out dir> [dump...]; no dump names means all.
 */
public final class Main {
    public static void main(String[] args) throws Exception {
        if (args.length < 1) {
            System.err.println("usage: Main <out dir> [blocks|state_ids|materials|actual_states|models|colors|biomes|geometry]...");
            System.exit(2);
        }
        File outDir = new File(args[0]);
        outDir.mkdirs();
        Bootstrap.register();

        boolean all = args.length == 1;
        if (all || contains(args,"sky")) {write(new File(outDir,"sky.json"),DumpSky.dump());}
        if (all || contains(args,"item_transforms")) {write(new File(outDir,"item_transforms.json"),DumpItemTransforms.dump());}
        if (all || contains(args, "item_models")) {
            Writer w = new OutputStreamWriter(new FileOutputStream(new File(outDir, "item_models.tsv")), "UTF-8");
            w.write(DumpItemModels.dump());
            w.close();
        }
        if (all || contains(args, "creative")) {
            DumpCreative.write(new File(outDir, "creative.tsv"));
        }
        if (all || contains(args, "items")) {
            write(new File(outDir, "items.json"), DumpItems.dump());
            write(new File(outDir, "enchantments.json"), DumpItems.enchantments(), false);
        }
        if (all || contains(args, "blocks")) {
            write(new File(outDir, "blocks.json"), DumpBlocks.dump());
        }
        if (all || contains(args, "state_ids")) {
            write(new File(outDir, "state_ids.json"), DumpStateIds.dump());
        }
        if (all || contains(args, "materials")) {
            write(new File(outDir, "materials.json"), DumpMaterials.dump());
        }
        if (all || contains(args, "actual_states")) {
            write(new File(outDir, "actual_states.json"), DumpActualStates.dump(), false);
        }
        if (all || contains(args, "models")) {
            write(new File(outDir, "models.json"), DumpModels.dump());
        }
        if (all || contains(args, "biomes")) {
            write(new File(outDir, "biomes.json"), DumpColors.biomes(), false);
        }
        if (all || contains(args, "colors")) {
            write(new File(outDir, "colors.json"), DumpColors.dump(), false);
        }
        if (all || contains(args, "furnace_rules")) {
            write(new File(outDir, "furnace_rules.json"), DumpContainers.furnaceRules(), false);
        }
        if (all || contains(args, "containers")) {
            write(new File(outDir, "containers.json"), DumpContainers.dump(), false);
        }
        if (all || contains(args, "movement")) {
            write(new File(outDir, "movement.json"), DumpMovement.dump(), false);
        }
        if (all || contains(args, "geometry")) {
            write(new File(outDir, "geometry.json"), DumpGeometry.dump(), false);
        }
    }

    private static boolean contains(String[] args, String name) {
        for (int i = 1; i < args.length; i++) {
            if (args[i].equals(name)) {
                return true;
            }
        }
        return false;
    }

    private static void write(File f, JsonElement e) throws Exception {
        write(f, e, true);
    }

    /** Large scene dumps are written compactly; pretty printing would multiply their size. */
    private static void write(File f, JsonElement e, boolean pretty) throws Exception {
        Writer w = new OutputStreamWriter(new FileOutputStream(f), "UTF-8");
        try {
            GsonBuilder gb = new GsonBuilder().serializeSpecialFloatingPointValues();
            if (pretty) {
                gb.setPrettyPrinting();
            }
            gb.create().toJson(e, w);
        } finally {
            w.close();
        }
        System.out.println("wrote " + f);
    }
}
