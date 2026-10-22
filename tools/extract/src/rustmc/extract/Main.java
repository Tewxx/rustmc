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
            System.err.println("usage: Main <out dir> [blocks]");
            System.exit(2);
        }
        File outDir = new File(args[0]);
        outDir.mkdirs();
        Bootstrap.register();

        boolean all = args.length == 1;
        if (all || contains(args, "blocks")) {
            write(new File(outDir, "blocks.json"), DumpBlocks.dump());
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
        Writer w = new OutputStreamWriter(new FileOutputStream(f), "UTF-8");
        try {
            new GsonBuilder().setPrettyPrinting().serializeSpecialFloatingPointValues().create().toJson(e, w);
        } finally {
            w.close();
        }
        System.out.println("wrote " + f);
    }
}
