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
            System.err.println("usage: Main <out dir> [blocks|state_ids|materials|actual_states]...");
            System.exit(2);
        }
        File outDir = new File(args[0]);
        outDir.mkdirs();
        Bootstrap.register();

        boolean all = args.length == 1;
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
