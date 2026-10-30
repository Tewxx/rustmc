import java.io.*;
import java.nio.file.*;
import java.util.*;
import java.util.jar.*;
import java.util.zip.*;
import org.objectweb.asm.*;
import org.objectweb.asm.commons.*;

/** Local-only experimental Forge-patched, SRG-named class jar. */
public class ShadowJar {
    public static void main(String[] a) throws Exception {
        if (a.length != 6) throw new IllegalArgumentException("vanilla.jar forge.jar joined.srg methods.csv fields.csv output.jar");
        Path vanilla = Paths.get(a[0]), forge = Paths.get(a[1]), output = Paths.get(a[5]);
        ForgeSpec.loadMappings(Paths.get(a[2]), Paths.get(a[3]), Paths.get(a[4]));
        Map<String, byte[]> classes = ForgeSpec.readJar(vanilla);
        Map<String, byte[]> forgeClasses = ForgeSpec.readJar(forge);
        ByteArrayOutputStream unpacked = new ByteArrayOutputStream();
        try (ZipFile z = new ZipFile(forge.toFile()); JarOutputStream jar = new JarOutputStream(unpacked)) {
            ZipEntry entry = z.getEntry("binpatches.pack.lzma");
            if (entry == null) throw new IOException("missing Forge binpatches");
            java.util.jar.Pack200.newUnpacker().unpack(new LZMA.LzmaInputStream(z.getInputStream(entry)), jar);
        }
        int patches = 0;
        net.minecraftforge.fml.repackage.com.nothome.delta.GDiffPatcher patcher = new net.minecraftforge.fml.repackage.com.nothome.delta.GDiffPatcher();
        try (JarInputStream jar = new JarInputStream(new ByteArrayInputStream(unpacked.toByteArray()))) {
            JarEntry entry;
            while ((entry = jar.getNextJarEntry()) != null) {
                if (!entry.getName().matches("binpatch/client/.*\\.binpatch")) continue;
                DataInputStream in = new DataInputStream(new ByteArrayInputStream(ForgeSpec.readAll(jar)));
                String name = in.readUTF(), source = in.readUTF().replace('.', '/'); in.readUTF();
                boolean exists = in.readBoolean(); int checksum = exists ? in.readInt() : 0;
                byte[] delta = new byte[in.readInt()]; in.readFully(delta);
                byte[] original = classes.get(source);
                if (exists && original == null) throw new IOException("missing patch source " + source);
                if (original == null) original = new byte[0];
                if (exists) { Adler32 adler = new Adler32(); adler.update(original); if ((int) adler.getValue() != checksum) throw new IOException("checksum mismatch: " + name); }
                classes.put(source, patcher.patch(original, delta)); patches++;
            }
        }
        for (Map.Entry<String, byte[]> e : forgeClasses.entrySet()) {
            String n = e.getKey();
            if (!n.startsWith("net/minecraftforge/") && !n.startsWith("paulscode") && !n.startsWith("ibxm")) classes.putIfAbsent(n, e.getValue());
        }
        for (byte[] bytes : classes.values()) ForgeSpec.indexNotch(ForgeSpec.parse(bytes));
        for (byte[] bytes : forgeClasses.values()) ForgeSpec.indexNotch(ForgeSpec.parse(bytes));
        Remapper remapper = new Remapper() {
            @Override public String map(String name) {
                String mapped = ForgeSpec.mapCls(name);
                if (!mapped.equals(name)) return mapped;
                // Forge can add nested classes absent from joined.srg. Their enclosing
                // vanilla class still has a mapping, so carry its mapping to the suffix.
                int inner = name.indexOf('$');
                if (inner < 0) return name;
                String outer = name.substring(0, inner);
                return ForgeSpec.mapCls(outer) + name.substring(inner);
            }
            @Override public String mapFieldName(String owner, String name, String desc) { return ForgeSpec.mapField(owner, name); }
            @Override public String mapMethodName(String owner, String name, String desc) { return ForgeSpec.mapMethod(owner, name, desc); }
        };
        Set<String> written = new HashSet<>();
        Path destination = output.toAbsolutePath();
        if (Files.exists(destination)) throw new FileAlreadyExistsException(destination.toString());
        Path temp = Files.createTempFile(destination.getParent(), ".rustmc-shadow-", ".tmp");
        try {
        try (JarOutputStream jar = new JarOutputStream(Files.newOutputStream(temp))) {
            for (byte[] bytes : classes.values()) {
                ClassReader reader = new ClassReader(bytes); ClassWriter writer = new ClassWriter(0);
                reader.accept(new RemappingClassAdapter(writer, remapper), ClassReader.EXPAND_FRAMES);
                String name = remapper.map(reader.getClassName()) + ".class";
                if (!written.add(name)) throw new IOException("duplicate mapped class " + name);
                JarEntry entry = new JarEntry(name); entry.setTime(0); jar.putNextEntry(entry); jar.write(writer.toByteArray()); jar.closeEntry();
            }
        }
        Files.move(temp, destination);
        } finally { Files.deleteIfExists(temp); }
        System.out.println("Applied " + patches + " client patches; wrote " + written.size() + " classes to " + output);
    }
}
