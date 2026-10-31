import java.io.*;
import java.nio.file.*;
import java.util.*;
import java.util.jar.*;
import java.util.zip.*;
import org.objectweb.asm.*;
import org.objectweb.asm.commons.*;

/**
 * Tiny MCP -> SRG reobfuscator for Forge 1.8.9 test mods (stands in for ForgeGradle's reobfJar).
 * Usage: Reobf joined.srg methods.csv fields.csv in.jar out.jar classpath-entry...
 * Classpath entries (dirs of MCP-named MC classes, Forge jar, libs) are used only to walk class hierarchies.
 * Member references and overriding declarations are renamed by resolving the declaring vanilla class.
 */
public class Reobf {
    static final Map<String, String> mcpToSrg = new HashMap<>(); // "owner.name desc" (methods) / "owner.name" (fields) -> srg
    static final Map<String, String[]> hier = new HashMap<>();
    static final List<File> cpDirs = new ArrayList<>();
    static final List<JarFile> cpJars = new ArrayList<>();
    static int renamed;

    static final Map<String, String> classMap = new HashMap<>(); // deobf mode: notch class -> final class

    /** Usage: Reobf [deobf] joined.srg methods.csv fields.csv in.jar out.jar classpath-entry...
     *  deobf: notch -> MCP names (used once to make a compile-time Forge jar; the universal jar references notch names). */
    public static void main(String[] args) throws Exception {
        final boolean deobf = args[0].equals("deobf");
        String[] a = deobf ? Arrays.copyOfRange(args, 1, args.length) : args;
        Map<String, String> mName = csv(a[1]), fName = csv(a[2]);
        for (String l : Files.readAllLines(Paths.get(a[0]))) {
            if (deobf) {
                if (!(l.startsWith("CL: ") || l.startsWith("MD: ") || l.startsWith("FD: "))) continue;
                String[] p = l.substring(4).split(" ");
                if (l.startsWith("CL: ")) classMap.put(p[0], p[1]);
                else if (l.startsWith("MD: ")) {
                    String srg = p[2].substring(p[2].lastIndexOf('/') + 1);
                    int i = p[0].lastIndexOf('/');
                    mcpToSrg.put(p[0].substring(0, i) + "." + p[0].substring(i + 1) + p[1], mName.getOrDefault(srg, srg));
                } else {
                    String srg = p[1].substring(p[1].lastIndexOf('/') + 1);
                    int i = p[0].lastIndexOf('/');
                    mcpToSrg.put(p[0].substring(0, i) + "." + p[0].substring(i + 1), fName.getOrDefault(srg, srg));
                }
            } else if (l.startsWith("MD: ")) {
                String[] p = l.substring(4).split(" ");
                int i = p[2].lastIndexOf('/');
                String srg = p[2].substring(i + 1), owner = p[2].substring(0, i);
                mcpToSrg.put(owner + "." + mName.getOrDefault(srg, srg) + p[3], srg);
            } else if (l.startsWith("FD: ")) {
                String[] p = l.substring(4).split(" ");
                int i = p[1].lastIndexOf('/');
                String srg = p[1].substring(i + 1), owner = p[1].substring(0, i);
                mcpToSrg.put(owner + "." + fName.getOrDefault(srg, srg), srg);
            }
        }
        for (int i = 5; i < a.length; i++) {
            File f = new File(a[i]);
            if (f.isDirectory()) cpDirs.add(f); else cpJars.add(new JarFile(f));
        }
        Remapper rm = new Remapper() {
            public String map(String t) { String r = deobf ? classMap.get(t) : null; return r == null ? t : r; }
            public String mapMethodName(String o, String n, String d) { String r = resolve(o, n + d); return r == null ? n : r; }
            public String mapFieldName(String o, String n, String d) { String r = resolve(o, n); return r == null ? n : r; }
        };
        try (JarFile in = new JarFile(a[3]); JarOutputStream out = new JarOutputStream(new FileOutputStream(a[4]))) {
            for (Enumeration<JarEntry> e = in.entries(); e.hasMoreElements();) {
                JarEntry je = e.nextElement();
                if (je.isDirectory()) continue;
                byte[] data = readAll(in.getInputStream(je));
                if (je.getName().endsWith(".class")) {
                    ClassReader cr = new ClassReader(data);
                    hier.put(cr.getClassName(), supers(cr));
                    ClassWriter cw = new ClassWriter(0);
                    cr.accept(new RemappingClassAdapter(cw, rm), ClassReader.EXPAND_FRAMES);
                    data = cw.toByteArray();
                }
                out.putNextEntry(new ZipEntry(je.getName()));
                out.write(data);
                out.closeEntry();
            }
        }
        System.err.println("reobf " + new File(a[3]).getName() + ": " + renamed + " member renames");
    }

    static String resolve(String owner, String key) {
        Deque<String> q = new ArrayDeque<>();
        Set<String> seen = new HashSet<>();
        q.add(owner);
        while (!q.isEmpty()) {
            String c = q.poll();
            if (!seen.add(c)) continue;
            String r = mcpToSrg.get(c + "." + key);
            if (r != null) { renamed++; return r; }
            String[] s = hierOf(c);
            if (s != null) q.addAll(Arrays.asList(s));
        }
        return null;
    }

    static String[] hierOf(String c) {
        if (hier.containsKey(c)) return hier.get(c);
        String[] r = null;
        try {
            byte[] b = find(c + ".class");
            if (b != null) r = supers(new ClassReader(b));
        } catch (IOException ex) { }
        hier.put(c, r);
        return r;
    }

    static String[] supers(ClassReader cr) {
        List<String> l = new ArrayList<>();
        if (cr.getSuperName() != null) l.add(cr.getSuperName());
        l.addAll(Arrays.asList(cr.getInterfaces()));
        return l.toArray(new String[0]);
    }

    static byte[] find(String name) throws IOException {
        for (File d : cpDirs) { File f = new File(d, name); if (f.isFile()) return Files.readAllBytes(f.toPath()); }
        for (JarFile j : cpJars) { ZipEntry z = j.getEntry(name); if (z != null) return readAll(j.getInputStream(z)); }
        return null;
    }

    static Map<String, String> csv(String f) throws IOException {
        Map<String, String> m = new HashMap<>();
        for (String l : Files.readAllLines(Paths.get(f))) { String[] p = l.split(",", 3); if (p.length > 1) m.put(p[0], p[1]); }
        return m;
    }

    static byte[] readAll(InputStream in) throws IOException {
        ByteArrayOutputStream o = new ByteArrayOutputStream();
        byte[] b = new byte[8192];
        int n;
        while ((n = in.read(b)) > 0) o.write(b, 0, n);
        in.close();
        return o.toByteArray();
    }
}
