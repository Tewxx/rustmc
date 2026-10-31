package rustmc.forge;

import java.io.*;
import java.util.*;
import net.minecraft.client.resources.IResource;
import net.minecraft.client.resources.IResourceManager;
import net.minecraft.client.resources.data.IMetadataSection;
import net.minecraft.util.ResourceLocation;

/**
 * Minecraft.getResourceManager: SimpleReloadableResourceManager's lookup order with Forge's mod packs.
 * The client's selected resource packs win, then mod jars (FMLFileResourcePack), then the vanilla jar.
 */
public final class Resources implements IResourceManager {
    public static final Resources INSTANCE = new Resources();

    private static byte[] read(String full, boolean base) {
        return (byte[]) Stub.call(base ? "rustmc/forge/Resources.readBase" : "rustmc/forge/Resources.readPacks", null, new Object[] { full });
    }

    private static byte[] mod(String full) {
        try (InputStream in = Resources.class.getClassLoader().getResourceAsStream(full)) {
            if (in == null) return null;
            ByteArrayOutputStream out = new ByteArrayOutputStream();
            byte[] buf = new byte[8192];
            for (int n; (n = in.read(buf)) > 0; ) out.write(buf, 0, n);
            return out.toByteArray();
        } catch (IOException e) {
            return null;
        }
    }

    /** Every layer's copy, lowest first (getAllResources). */
    private static List<byte[]> layers(String full) {
        List<byte[]> all = new ArrayList<>();
        byte[] b;
        if ((b = read(full, true)) != null) all.add(b);
        if ((b = mod(full)) != null) all.add(b);
        if ((b = read(full, false)) != null) all.add(b);
        return all;
    }

    private static String full(ResourceLocation loc) { return "assets/" + loc.func_110624_b() + "/" + loc.func_110623_a(); }

    @Override public Set<String> func_135055_a() { return new HashSet<>(Arrays.asList("minecraft", "realms")); }

    @Override public IResource func_110536_a(ResourceLocation loc) throws IOException {
        List<byte[]> all = layers(full(loc));
        if (all.isEmpty()) throw new FileNotFoundException(loc.toString());
        List<byte[]> meta = layers(full(loc) + ".mcmeta");
        return new Res(loc, all.get(all.size() - 1), meta.isEmpty() ? null : meta.get(meta.size() - 1));
    }

    @Override public List<IResource> func_135056_b(ResourceLocation loc) throws IOException {
        List<IResource> out = new ArrayList<>();
        for (byte[] b : layers(full(loc))) out.add(new Res(loc, b, null));
        if (out.isEmpty()) throw new FileNotFoundException(loc.toString());
        return out;
    }

    static final class Res implements IResource {
        private final ResourceLocation loc; private final byte[] data, meta;
        Res(ResourceLocation loc, byte[] data, byte[] meta) { this.loc = loc; this.data = data; this.meta = meta; }
        @Override public ResourceLocation func_177241_a() { return loc; }
        @Override public InputStream func_110527_b() { return new ByteArrayInputStream(data); }
        @Override public boolean func_110528_c() { return meta != null; }
        // No IMetadataSerializer on this side: sections are parsed by the Rust texture/animation loaders.
        @Override public <T extends IMetadataSection> T func_110526_a(String section) { return null; }
        @Override public String func_177240_d() { return "rustmc"; }
    }
}
