import java.io.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import java.util.jar.*;
import java.util.zip.*;
import org.objectweb.asm.*;
import org.objectweb.asm.tree.*;

/**
 * Dumps the Forge 1.8.9 API surface (Forge/FML, vanilla as patched by Forge, LaunchWrapper, Mixin) and the
 * event catalogue from compiled jars. Records signatures only; no method bodies are copied.
 * Usage: see run.sh. Args: outDir forgeJar vanillaJar joinedSrg methodsCsv fieldsCsv lwJar mixinJar
 */
public class ForgeSpec {
    // ---------- mappings ----------
    static Map<String, String> cls = new HashMap<>();          // notch -> srg (internal names)
    static Map<String, String> rcls = new HashMap<>();
    static Map<String, String> fld = new HashMap<>();          // notchOwner/name -> srgName
    static Map<String, String> mth = new HashMap<>();          // notchOwner/name desc -> srgName
    static Map<String, String> mcpM = new HashMap<>(), mcpF = new HashMap<>();
    static Map<String, String> notchSuper = new HashMap<>();   // notch class -> notch super
    static Map<String, List<String>> notchIfs = new HashMap<>();

    static String mapCls(String n) { if (n == null) return null; if (n.startsWith("[")) return mapDesc(n); String r = cls.get(n); return r != null ? r : n; }
    static String mapDesc(String d) {
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < d.length(); i++) {
            char c = d.charAt(i);
            if (c == 'L') { int e = d.indexOf(';', i); sb.append('L').append(mapCls(d.substring(i + 1, e))).append(';'); i = e; }
            else sb.append(c);
        }
        return sb.toString();
    }
    static String mapField(String owner, String name) {
        for (String o = owner; o != null; o = notchSuper.get(o)) { String r = fld.get(o + "/" + name); if (r != null) return r; }
        return name;
    }
    static String mapMethod(String owner, String name, String desc) {
        String r = lookupM(owner, name, desc, new HashSet<String>());
        return r != null ? r : name;
    }
    static String lookupM(String owner, String name, String desc, Set<String> seen) {
        if (owner == null || !seen.add(owner)) return null;
        String r = mth.get(owner + "/" + name + desc);
        if (r != null) return r;
        r = lookupM(notchSuper.get(owner), name, desc, seen);
        if (r != null) return r;
        List<String> ifs = notchIfs.get(owner);
        if (ifs != null) for (String i : ifs) { r = lookupM(i, name, desc, seen); if (r != null) return r; }
        return null;
    }
    static String mcpMethod(String srg) { String r = mcpM.get(srg); return r != null ? r : srg; }
    static String mcpField(String srg) { String r = mcpF.get(srg); return r != null ? r : srg; }

    static void loadMappings(Path srg, Path mcsv, Path fcsv) throws IOException {
        for (String l : Files.readAllLines(srg)) {
            String[] p = l.split(" ");
            if (p[0].equals("CL:")) { cls.put(p[1], p[2]); rcls.put(p[2], p[1]); }
            else if (p[0].equals("FD:")) { fld.put(p[1], p[2].substring(p[2].lastIndexOf('/') + 1)); }
            else if (p[0].equals("MD:")) { mth.put(p[1] + p[2], p[3].substring(p[3].lastIndexOf('/') + 1)); }
        }
        for (String l : Files.readAllLines(mcsv)) { String[] p = l.split(",", 3); if (p.length > 1) mcpM.put(p[0], p[1]); }
        for (String l : Files.readAllLines(fcsv)) { String[] p = l.split(",", 3); if (p.length > 1) mcpF.put(p[0], p[1]); }
    }

    // ---------- model ----------
    static class M { String name, srg, mcp, desc; int acc, accPre; boolean forge; String status = ""; int[] lines; }
    static class C {
        String name, notch, sup, src; List<String> ifs = new ArrayList<>(); int acc, accPre; List<M> fields = new ArrayList<>(), methods = new ArrayList<>();
        ClassNode node; boolean patched, isNew; String sourceFile;
    }

    static String mods(int a, boolean isMethod) {
        List<String> l = new ArrayList<>();
        if ((a & Opcodes.ACC_PUBLIC) != 0) l.add("public");
        if ((a & Opcodes.ACC_PROTECTED) != 0) l.add("protected");
        if ((a & Opcodes.ACC_PRIVATE) != 0) l.add("private");
        if ((a & Opcodes.ACC_STATIC) != 0) l.add("static");
        if ((a & Opcodes.ACC_FINAL) != 0) l.add("final");
        if ((a & Opcodes.ACC_ABSTRACT) != 0) l.add("abstract");
        if (isMethod && (a & Opcodes.ACC_SYNCHRONIZED) != 0) l.add("synchronized");
        if (isMethod && (a & Opcodes.ACC_NATIVE) != 0) l.add("native");
        if (!isMethod && (a & Opcodes.ACC_VOLATILE) != 0) l.add("volatile");
        if (!isMethod && (a & Opcodes.ACC_TRANSIENT) != 0) l.add("transient");
        return String.join(" ", l);
    }
    static String clsMods(int a) {
        String s = mods(a & ~Opcodes.ACC_ABSTRACT, false);
        if ((a & Opcodes.ACC_INTERFACE) != 0) s += " interface"; else if ((a & Opcodes.ACC_ENUM) != 0) s += " enum"; else if ((a & Opcodes.ACC_ABSTRACT) != 0) s += " abstract";
        if ((a & Opcodes.ACC_ANNOTATION) != 0) s += " @annotation";
        return s.trim();
    }

    static byte[] readAll(InputStream in) throws IOException {
        ByteArrayOutputStream o = new ByteArrayOutputStream(); byte[] b = new byte[8192]; int n;
        while ((n = in.read(b)) > 0) o.write(b, 0, n); return o.toByteArray();
    }
    static Map<String, byte[]> readJar(Path p) throws IOException {
        Map<String, byte[]> m = new LinkedHashMap<>();
        try (ZipFile z = new ZipFile(p.toFile())) {
            for (Enumeration<? extends ZipEntry> e = z.entries(); e.hasMoreElements();) {
                ZipEntry ze = e.nextElement();
                if (ze.isDirectory() || !ze.getName().endsWith(".class") || ze.getName().startsWith("META-INF/")) continue;
                m.put(ze.getName().substring(0, ze.getName().length() - 6), readAll(z.getInputStream(ze)));
            }
        }
        return m;
    }
    static ClassNode parse(byte[] b) {
        ClassNode cn = new ClassNode();
        new ClassReader(b).accept(cn, ClassReader.SKIP_FRAMES);
        return cn;
    }
    static void indexNotch(ClassNode cn) { notchSuper.put(cn.name, cn.superName); notchIfs.put(cn.name, cn.interfaces == null ? new ArrayList<String>() : cn.interfaces); }

    /** Build C from node. remap=true: names are notch and get mapped to SRG. */
    @SuppressWarnings("unchecked")
    static C build(ClassNode cn, boolean remap, String src) {
        C c = new C(); c.node = cn; c.src = src; c.notch = cn.name; c.sourceFile = cn.sourceFile;
        c.name = remap ? mapCls(cn.name) : cn.name; c.sup = remap ? mapCls(cn.superName) : cn.superName; c.acc = c.accPre = cn.access;
        for (String i : (List<String>) cn.interfaces) c.ifs.add(remap ? mapCls(i) : i);
        for (FieldNode f : (List<FieldNode>) cn.fields) {
            M m = new M(); m.name = f.name; m.desc = remap ? mapDesc(f.desc) : f.desc; m.acc = m.accPre = f.access;
            if (remap) { m.srg = mapField(cn.name, f.name); m.name = m.srg; m.mcp = mcpField(m.srg); } else { m.srg = f.name; m.mcp = f.name; }
            c.fields.add(m);
        }
        for (MethodNode mn : (List<MethodNode>) cn.methods) {
            M m = new M(); m.desc = remap ? mapDesc(mn.desc) : mn.desc; m.acc = m.accPre = mn.access;
            if (remap) { m.srg = mapMethod(cn.name, mn.name, mn.desc); m.name = m.srg; m.mcp = mcpMethod(m.srg); } else { m.srg = mn.name; m.name = mn.name; m.mcp = mn.name; }
            c.methods.add(m);
        }
        return c;
    }

    // ---------- access transformers ----------
    static List<String[]> atRules = new ArrayList<>();
    static void loadAT(String text) {
        for (String l : text.split("\n")) {
            int h = l.indexOf('#'); if (h >= 0) l = l.substring(0, h);
            l = l.trim(); if (l.isEmpty()) continue;
            String[] p = l.split("\\s+");
            if (p.length < 2) continue;
            atRules.add(p);
        }
    }
    static int applyAcc(int acc, String spec) {
        boolean pf = spec.endsWith("+f"), mf = spec.endsWith("-f");
        String a = spec.replace("+f", "").replace("-f", "");
        int vis = acc & ~(Opcodes.ACC_PUBLIC | Opcodes.ACC_PROTECTED | Opcodes.ACC_PRIVATE);
        int cur = acc & (Opcodes.ACC_PUBLIC | Opcodes.ACC_PROTECTED | Opcodes.ACC_PRIVATE);
        int nv;
        switch (a) { case "public": nv = Opcodes.ACC_PUBLIC; break; case "protected": nv = Opcodes.ACC_PROTECTED; if (cur == Opcodes.ACC_PUBLIC) nv = cur; break; case "private": nv = Opcodes.ACC_PRIVATE; break; default: nv = 0; if (cur == Opcodes.ACC_PUBLIC || cur == Opcodes.ACC_PROTECTED) nv = cur; }
        int r = vis | nv;
        if (pf) r |= Opcodes.ACC_FINAL; if (mf) r &= ~Opcodes.ACC_FINAL;
        return r;
    }
    static int atApplied = 0;
    static void applyATs(Map<String, C> byName) {
        for (String[] r : atRules) {
            String cn = r[1].replace('.', '/');
            C c = byName.get(cn);
            if (c == null) continue;
            if (r.length == 2) { c.acc = applyAcc(c.acc, r[0]); atApplied++; continue; }
            String mem = r[2];
            // method rule: name(desc)ret with dotted class names
            int pi = mem.indexOf('(');
            if (pi < 0) {
                for (M f : c.fields) if (mem.equals("*") || f.srg.equals(mem)) { f.acc = applyAcc(f.acc, r[0]); atApplied++; }
            } else {
                String n = mem.substring(0, pi), d = mem.substring(pi).replace('.', '/');
                for (M m : c.methods) if ((n.equals("*") && d.equals("()")) || (m.srg.equals(n) && (d.equals("") || m.desc.equals(d)))) { m.acc = applyAcc(m.acc, r[0]); atApplied++; }
                if (n.equals("*")) for (M m : c.methods) { m.acc = applyAcc(m.acc, r[0]); atApplied++; }
            }
        }
    }

    // ---------- JSON ----------
    static String q(String s) {
        if (s == null) return "null";
        StringBuilder sb = new StringBuilder("\"");
        for (char ch : s.toCharArray()) {
            switch (ch) { case '"': sb.append("\\\""); break; case '\\': sb.append("\\\\"); break; case '\n': sb.append("\\n"); break;
                default: if (ch < 0x20) sb.append(String.format("\\u%04x", (int) ch)); else sb.append(ch); }
        }
        return sb.append('"').toString();
    }
    static boolean vis(int a) { return (a & (Opcodes.ACC_PUBLIC | Opcodes.ACC_PROTECTED)) != 0; }
    static String memberJson(M m, boolean isMethod) {
        StringBuilder sb = new StringBuilder("{\"n\":").append(q(m.srg));
        if (m.mcp != null && !m.mcp.equals(m.srg)) sb.append(",\"mcp\":").append(q(m.mcp));
        sb.append(",\"d\":").append(q(m.desc)).append(",\"m\":").append(q(mods(m.acc, isMethod)));
        if (!m.status.isEmpty()) sb.append(",\"forge\":").append(q(m.status));
        return sb.append('}').toString();
    }
    static String classJson(C c) {
        StringBuilder sb = new StringBuilder("{\"name\":").append(q(c.name));
        if (c.notch != null && !c.notch.equals(c.name)) sb.append(",\"notch\":").append(q(c.notch));
        sb.append(",\"src\":").append(q(c.src)).append(",\"m\":").append(q(clsMods(c.acc))).append(",\"super\":").append(q(c.sup)).append(",\"ifs\":[");
        for (int i = 0; i < c.ifs.size(); i++) sb.append(i > 0 ? "," : "").append(q(c.ifs.get(i)));
        sb.append("]");
        if (c.patched) sb.append(",\"forgePatched\":true");
        if (c.isNew) sb.append(",\"forgeNew\":true");
        sb.append(",\"fields\":[");
        boolean first = true;
        for (M f : c.fields) if (vis(f.acc)) { sb.append(first ? "" : ",").append(memberJson(f, false)); first = false; }
        sb.append("],\"methods\":[");
        first = true;
        for (M m : c.methods) if (vis(m.acc)) { sb.append(first ? "" : ",").append(memberJson(m, true)); first = false; }
        return sb.append("]}").toString();
    }
    static void writeClasses(Path out, Collection<C> cs) throws IOException {
        List<C> l = new ArrayList<>(cs); Collections.sort(l, (a, b) -> a.name.compareTo(b.name));
        try (Writer w = Files.newBufferedWriter(out, StandardCharsets.UTF_8)) {
            w.write("[\n");
            boolean first = true;
            for (C c : l) { if (!vis(c.acc) && !c.name.contains("$")) { /* keep only public/protected top-level API; package-private classes skipped */ continue; } if (!first) w.write(",\n"); first = false; w.write(classJson(c)); }
            w.write("\n]\n");
        }
    }

    // ---------- main ----------
    static final String EVENT = "net/minecraftforge/fml/common/eventhandler/Event";
    static final String BUS = "net/minecraftforge/fml/common/eventhandler/EventBus";

    public static void main(String[] a) throws Exception {
        Path out = Paths.get(a[0]); Files.createDirectories(out);
        Path forgeJar = Paths.get(a[1]), vanJar = Paths.get(a[2]);
        loadMappings(Paths.get(a[3]), Paths.get(a[4]), Paths.get(a[5]));
        Map<String, byte[]> vanilla = readJar(vanJar), forge = readJar(forgeJar);
        Map<String, Object> summary = new LinkedHashMap<>();

        // index notch hierarchy (vanilla + forge-added obf classes)
        Map<String, ClassNode> vanNodes = new HashMap<>();
        for (Map.Entry<String, byte[]> e : vanilla.entrySet()) { ClassNode cn = parse(e.getValue()); vanNodes.put(e.getKey(), cn); indexNotch(cn); }

        // load binpatches
        List<Object[]> patches = new ArrayList<>(); // name, source, target, exists, checksum, patch
        byte[] lz = null;
        try (ZipFile z = new ZipFile(forgeJar.toFile())) { lz = readAll(z.getInputStream(z.getEntry("binpatches.pack.lzma"))); }
        ByteArrayOutputStream jarBytes = new ByteArrayOutputStream();
        try (JarOutputStream jos = new JarOutputStream(jarBytes)) {
            java.util.jar.Pack200.newUnpacker().unpack(new LZMA.LzmaInputStream(new ByteArrayInputStream(lz)), jos);
        }
        Map<String, List<Object[]>> patchBySrc = new LinkedHashMap<>();
        int nPatch = 0, nCsumBad = 0;
        try (JarInputStream jis = new JarInputStream(new ByteArrayInputStream(jarBytes.toByteArray()))) {
            JarEntry je;
            while ((je = jis.getNextJarEntry()) != null) {
                if (!je.getName().matches("binpatch/client/.*\\.binpatch")) continue;
                DataInputStream in = new DataInputStream(new ByteArrayInputStream(readAll(jis)));
                String name = in.readUTF(), src = in.readUTF(), tgt = in.readUTF(); boolean ex = in.readBoolean(); int cs = ex ? in.readInt() : 0;
                byte[] pb = new byte[in.readInt()]; in.readFully(pb);
                patchBySrc.computeIfAbsent(src.replace('.', '/'), k -> new ArrayList<>()).add(new Object[]{name, src, tgt, ex, cs, pb});
                nPatch++;
            }
        }
        net.minecraftforge.fml.repackage.com.nothome.delta.GDiffPatcher gd = new net.minecraftforge.fml.repackage.com.nothome.delta.GDiffPatcher();
        Map<String, byte[]> patchedBytes = new LinkedHashMap<>();
        for (Map.Entry<String, List<Object[]>> e : patchBySrc.entrySet()) {
            byte[] data = vanilla.containsKey(e.getKey()) ? vanilla.get(e.getKey()) : new byte[0];
            for (Object[] p : e.getValue()) {
                if ((Boolean) p[3]) {
                    Adler32 ad = new Adler32(); ad.update(data); if ((int) ad.getValue() != (Integer) p[4]) nCsumBad++;
                }
                data = gd.patch(data, (byte[]) p[5]);
            }
            patchedBytes.put(e.getKey(), data);
        }
        summary.put("binpatches", nPatch); summary.put("binpatchChecksumMismatches", nCsumBad);
        // forge-added root-level obf classes (inner classes of patched vanilla) not covered by patch set
        Map<String, byte[]> forgeNotchExtras = new LinkedHashMap<>();
        Map<String, byte[]> forgeOwn = new LinkedHashMap<>();
        for (Map.Entry<String, byte[]> e : forge.entrySet()) {
            if (e.getKey().startsWith("net/minecraftforge/") || e.getKey().startsWith("paulscode") || e.getKey().startsWith("ibxm")) forgeOwn.put(e.getKey(), e.getValue());
            else forgeNotchExtras.put(e.getKey(), e.getValue());
        }
        for (Map.Entry<String, byte[]> e : forgeNotchExtras.entrySet()) { if (!patchedBytes.containsKey(e.getKey())) { ClassNode cn = parse(e.getValue()); indexNotch(cn); patchedBytes.put(e.getKey(), e.getValue()); } }
        for (String k : patchedBytes.keySet()) { ClassNode cn = parse(patchedBytes.get(k)); indexNotch(cn); }
        for (byte[] b : forgeOwn.values()) indexNotch(parse(b));

        // build class models
        Map<String, C> vanOrig = new LinkedHashMap<>(), vanPatched = new LinkedHashMap<>(), forgeC = new LinkedHashMap<>();
        for (Map.Entry<String, ClassNode> e : vanNodes.entrySet()) { C c = build(e.getValue(), true, "minecraft"); vanOrig.put(c.name, c); }
        for (Map.Entry<String, byte[]> e : patchedBytes.entrySet()) {
            if (e.getValue().length == 0) continue;
            C c = build(parse(e.getValue()), true, "minecraft"); c.patched = vanilla.containsKey(e.getKey()); c.isNew = !c.patched; vanPatched.put(c.name, c);
        }
        for (Map.Entry<String, byte[]> e : forgeOwn.entrySet()) { C c = build(parse(e.getValue()), true, e.getKey().startsWith("net/minecraftforge/fml") ? "fml" : "forge"); forgeC.put(c.name, c); }

        // diff patched vs original
        StringBuilder diff = new StringBuilder("[\n"); boolean firstD = true;
        int dNewCls = 0, dPatchedCls = 0, dAddM = 0, dAddF = 0, dChgM = 0;
        Map<String, C> finalVan = new LinkedHashMap<>(vanOrig);
        for (C pc : vanPatched.values()) {
            C oc = vanOrig.get(pc.name);
            StringBuilder d = new StringBuilder("{\"class\":" + q(pc.name) + ",\"notch\":" + q(pc.notch));
            if (oc == null) {
                dNewCls++;
                d.append(",\"new\":true,\"super\":" + q(pc.sup) + ",\"addedMethods\":" + pc.methods.size() + ",\"addedFields\":" + pc.fields.size() + "}");
            } else {
                dPatchedCls++;
                List<String> am = new ArrayList<>(), af = new ArrayList<>(), rm = new ArrayList<>(), rf = new ArrayList<>(), ch = new ArrayList<>();
                Map<String, M> om = new HashMap<>(), of = new HashMap<>();
                for (M m : oc.methods) om.put(m.srg + m.desc, m); for (M f : oc.fields) of.put(f.srg, f);
                Set<String> seenM = new HashSet<>(), seenF = new HashSet<>();
                for (M m : pc.methods) {
                    seenM.add(m.srg + m.desc); M o = om.get(m.srg + m.desc);
                    if (o == null) { m.status = "+"; am.add(memberJson(m, true)); dAddM++; }
                    else if (o.acc != m.acc) { m.status = "~"; ch.add("{\"n\":" + q(m.srg) + ",\"mcp\":" + q(m.mcp) + ",\"d\":" + q(m.desc) + ",\"was\":" + q(mods(o.acc, true)) + ",\"now\":" + q(mods(m.acc, true)) + "}"); dChgM++; }
                }
                for (M f : pc.fields) { seenF.add(f.srg); M o = of.get(f.srg); if (o == null) { f.status = "+"; af.add(memberJson(f, false)); dAddF++; } else if (o.acc != f.acc) { f.status = "~"; ch.add("{\"field\":" + q(f.srg) + ",\"mcp\":" + q(f.mcp) + ",\"was\":" + q(mods(o.acc, false)) + ",\"now\":" + q(mods(f.acc, false)) + "}"); dChgM++; } }
                for (M m : oc.methods) if (!seenM.contains(m.srg + m.desc)) rm.add(memberJson(m, true));
                for (M f : oc.fields) if (!seenF.contains(f.srg)) rf.add(memberJson(f, false));
                List<String> ai = new ArrayList<>(pc.ifs); ai.removeAll(oc.ifs); List<String> ri = new ArrayList<>(oc.ifs); ri.removeAll(pc.ifs);
                d.append(",\"super\":" + (pc.sup.equals(oc.sup) ? "null" : q(pc.sup)) + ",\"addedInterfaces\":" + jl(ai) + ",\"removedInterfaces\":" + jl(ri)
                        + ",\"classAccess\":" + (pc.acc == oc.acc ? "null" : q(clsMods(oc.acc) + " -> " + clsMods(pc.acc)))
                        + ",\"addedMethods\":[" + String.join(",", am) + "],\"addedFields\":[" + String.join(",", af) + "],\"removedMethods\":[" + String.join(",", rm)
                        + "],\"removedFields\":[" + String.join(",", rf) + "],\"accessChanged\":[" + String.join(",", ch) + "]}");
            }
            diff.append(firstD ? "" : ",\n").append(d); firstD = false;
            finalVan.put(pc.name, pc);
        }
        diff.append("\n]\n");
        Files.write(out.resolve("vanilla-patch-diff.json"), diff.toString().getBytes(StandardCharsets.UTF_8));
        summary.put("patchedVanillaClasses", dPatchedCls); summary.put("forgeNewVanillaPackageClasses", dNewCls);
        summary.put("patchAddedMethods", dAddM); summary.put("patchAddedFields", dAddF); summary.put("patchAccessChanges", dChgM);

        // ATs
        try (ZipFile z = new ZipFile(forgeJar.toFile())) {
            for (String n : new String[]{"forge_at.cfg", "fml_at.cfg"}) { ZipEntry ze = z.getEntry(n); if (ze != null) loadAT(new String(readAll(z.getInputStream(ze)), StandardCharsets.UTF_8)); }
        }
        StringBuilder atj = new StringBuilder("[\n"); for (int i = 0; i < atRules.size(); i++) atj.append(i > 0 ? ",\n" : "").append(" ").append(jl(Arrays.asList(atRules.get(i)))); atj.append("\n]\n");
        Files.write(out.resolve("access-transformers.json"), atj.toString().getBytes(StandardCharsets.UTF_8));
        Map<String, C> all = new LinkedHashMap<>(); all.putAll(finalVan); all.putAll(forgeC);
        applyATs(all);
        summary.put("atRules", atRules.size());

        writeClasses(out.resolve("api-vanilla-patched.json"), finalVan.values());
        writeClasses(out.resolve("api-forge-fml.json"), forgeC.values());
        // launchwrapper / mixin
        int lw = 0, mx = 0;
        for (int i = 6; i < a.length; i++) {
            Map<String, C> m = new LinkedHashMap<>();
            for (Map.Entry<String, byte[]> e : readJar(Paths.get(a[i])).entrySet()) {
                try { C c = build(parse(e.getValue()), false, i == 6 ? "launchwrapper" : "mixin"); m.put(c.name, c); } catch (Exception ex) { System.err.println("skip " + e.getKey() + ": " + ex); }
            }
            writeClasses(out.resolve(i == 6 ? "api-launchwrapper.json" : "api-mixin.json"), m.values());
            summary.put(i == 6 ? "launchwrapperClasses" : "mixinClasses", m.size());
        }
        int pubVan = 0, pubForge = 0, mV = 0, mF = 0;
        for (C c : finalVan.values()) if (vis(c.acc)) { pubVan++; for (M m : c.methods) if (vis(m.acc)) mV++; }
        for (C c : forgeC.values()) if (vis(c.acc)) { pubForge++; for (M m : c.methods) if (vis(m.acc)) mF++; }
        summary.put("vanillaPublicClasses", pubVan); summary.put("vanillaPublicMethods", mV); summary.put("forgeFmlPublicClasses", pubForge); summary.put("forgeFmlPublicMethods", mF);

        EventScan.run(out, all, forgeC, finalVan, summary);

        StringBuilder sj = new StringBuilder("{\n"); int i = 0;
        for (Map.Entry<String, Object> e : summary.entrySet()) sj.append(i++ > 0 ? ",\n" : "").append("  ").append(q(e.getKey())).append(": ").append(e.getValue() instanceof Number ? e.getValue().toString() : q(String.valueOf(e.getValue())));
        sj.append("\n}\n");
        Files.write(out.resolve("summary.json"), sj.toString().getBytes(StandardCharsets.UTF_8));
        System.out.println(sj);
    }
    static String jl(List<String> l) { StringBuilder sb = new StringBuilder("["); for (int i = 0; i < l.size(); i++) sb.append(i > 0 ? "," : "").append(q(l.get(i))); return sb.append("]").toString(); }
}
