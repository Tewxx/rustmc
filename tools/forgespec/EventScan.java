import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import org.objectweb.asm.Opcodes;
import org.objectweb.asm.tree.*;

/** Finds every Event subclass and where (which methods) construct/post it, with transitive vanilla callers. */
public class EventScan {
    static String q(String s) { return ForgeSpec.q(s); }

    static class Site { String cls, method, mcp, desc, bus = "?", file; int line; boolean posts; String kind; }
    static class Ref { String cls, method, mcp, desc; int line; }

    @SuppressWarnings("unchecked")
    static void run(Path out, Map<String, ForgeSpec.C> all, Map<String, ForgeSpec.C> forgeC, Map<String, ForgeSpec.C> van, Map<String, Object> summary) throws Exception {
        // event classes
        Map<String, ForgeSpec.C> events = new TreeMap<>();
        for (ForgeSpec.C c : all.values()) {
            if (c.name.equals(ForgeSpec.EVENT)) continue;
            String s = c.sup; int guard = 0;
            while (s != null && guard++ < 30) { if (s.equals(ForgeSpec.EVENT)) { events.put(c.name, c); break; } ForgeSpec.C p = all.get(s); s = p == null ? null : p.sup; }
        }
        // scan set
        List<ForgeSpec.C> scan = new ArrayList<>();
        for (ForgeSpec.C c : forgeC.values()) scan.add(c);
        for (ForgeSpec.C c : van.values()) if (c.patched || c.isNew) scan.add(c);

        Map<String, List<Site>> constructs = new HashMap<>();   // event -> sites
        Map<String, Set<String>> callers = new HashMap<>();      // callee key -> caller keys
        Map<String, Ref> refs = new HashMap<>();
        for (ForgeSpec.C c : scan) {
            ClassNode cn = c.node;
            for (MethodNode mn : (List<MethodNode>) cn.methods) {
                String msrg = ForgeSpec.mapMethod(cn.name, mn.name, mn.desc);
                String mdesc = ForgeSpec.mapDesc(mn.desc);
                String key = c.name + "." + msrg + mdesc;
                Ref r = new Ref(); r.cls = c.name; r.method = msrg; r.mcp = ForgeSpec.mcpMethod(msrg); r.desc = mdesc; refs.put(key, r);
                int line = -1;
                List<String> lastBus = new ArrayList<>();
                Set<String> newEvents = new LinkedHashSet<>();
                Map<String, Integer> newLine = new HashMap<>(), newIdx = new HashMap<>();
                List<Object[]> postAt = new ArrayList<>();
                String bus = "?"; boolean posts = false; int idx = -1;
                for (AbstractInsnNode in : mn.instructions.toArray()) {
                    idx++;
                    if (in instanceof LineNumberNode) line = ((LineNumberNode) in).line;
                    else if (in instanceof FieldInsnNode && in.getOpcode() == Opcodes.GETSTATIC) {
                        FieldInsnNode f = (FieldInsnNode) in;
                        if (f.name.equals("EVENT_BUS") || f.name.equals("TERRAIN_GEN_BUS") || f.name.equals("ORE_GEN_BUS")) bus = f.owner.endsWith("MinecraftForge") ? "MinecraftForge." + f.name : f.owner + "." + f.name;
                    } else if (in instanceof TypeInsnNode && in.getOpcode() == Opcodes.NEW) {
                        String t = ForgeSpec.mapCls(((TypeInsnNode) in).desc);
                        if (events.containsKey(t) && newEvents.add(t)) { newLine.put(t, line); newIdx.put(t, idx); }
                    } else if (in instanceof MethodInsnNode) {
                        MethodInsnNode m = (MethodInsnNode) in;
                        String o = ForgeSpec.mapCls(m.owner), nm = ForgeSpec.mapMethod(m.owner, m.name, m.desc), d = ForgeSpec.mapDesc(m.desc);
                        if (m.name.equals("bus") && o.equals("net/minecraftforge/fml/common/FMLCommonHandler")) bus = "FMLCommonHandler.bus()";
                        if (o.equals(ForgeSpec.BUS) && m.name.equals("post")) { posts = true; postAt.add(new Object[]{idx, bus}); }
                        callers.computeIfAbsent(o + "." + nm + d, k -> new LinkedHashSet<>()).add(key);
                        Ref cr = refs.get(key); if (cr != null && cr.line == 0) cr.line = line;
                    }
                }
                for (String ev : newEvents) {
                    String b2 = "?"; boolean p2 = false;
                    for (Object[] pa : postAt) if ((Integer) pa[0] > newIdx.get(ev)) { b2 = (String) pa[1]; p2 = true; break; }
                    Site s = new Site(); s.cls = c.name; s.method = msrg; s.mcp = r.mcp; s.desc = mdesc; s.line = newLine.get(ev); s.bus = b2; s.posts = p2;
                    String outer = c.name.contains("$") ? c.name.substring(0, c.name.indexOf('$')) : c.name;
                    if (c.src.equals("minecraft")) { s.kind = "patched-vanilla"; s.file = "patches/minecraft/" + outer + ".java.patch"; }
                    else { s.kind = "forge"; s.file = "src/main/java/" + outer + ".java"; }
                    constructs.computeIfAbsent(ev, k -> new ArrayList<>()).add(s);
                }
            }
        }

        StringBuilder sb = new StringBuilder("[\n"); boolean first = true;
        int fired = 0, canc = 0, res = 0;
        for (ForgeSpec.C e : events.values()) {
            boolean cancelable = false, hasResult = false; String s = e.name; int guard = 0;
            while (s != null && guard++ < 30) {
                ForgeSpec.C p = all.get(s); if (p == null) break;
                if (p.node.visibleAnnotations != null) for (AnnotationNode an : (List<AnnotationNode>) p.node.visibleAnnotations) {
                    if (an.desc.equals("Lnet/minecraftforge/fml/common/eventhandler/Cancelable;")) cancelable = true;
                    if (an.desc.equals("Lnet/minecraftforge/fml/common/eventhandler/Event$HasResult;")) hasResult = true;
                }
                s = p.sup;
            }
            boolean ownCancel = false, ownRes = false;
            if (e.node.visibleAnnotations != null) for (AnnotationNode an : (List<AnnotationNode>) e.node.visibleAnnotations) {
                if (an.desc.equals("Lnet/minecraftforge/fml/common/eventhandler/Cancelable;")) ownCancel = true;
                if (an.desc.equals("Lnet/minecraftforge/fml/common/eventhandler/Event$HasResult;")) ownRes = true;
            }
            if (cancelable) canc++; if (hasResult) res++;
            StringBuilder o = new StringBuilder("{\"event\":" + q(e.name) + ",\"super\":" + q(e.sup) + ",\"abstract\":" + ((e.acc & Opcodes.ACC_ABSTRACT) != 0)
                    + ",\"cancelable\":" + cancelable + ",\"cancelableOwnAnnotation\":" + ownCancel + ",\"hasResult\":" + hasResult + ",\"hasResultOwnAnnotation\":" + ownRes
                    + ",\"package\":" + q(e.name.substring(0, e.name.lastIndexOf('/'))) + ",\"src\":" + q(e.src));
            o.append(",\"fields\":[");
            boolean f1 = true;
            for (ForgeSpec.M f : e.fields) if (ForgeSpec.vis(f.acc)) { o.append(f1 ? "" : ",").append(ForgeSpec.memberJson(f, false)); f1 = false; }
            o.append("],\"constructors\":[");
            f1 = true;
            for (ForgeSpec.M m : e.methods) if (m.srg.equals("<init>") && ForgeSpec.vis(m.acc)) { o.append(f1 ? "" : ",").append(q(m.desc)); f1 = false; }
            o.append("],\"sites\":[");
            List<Site> sites = constructs.get(e.name);
            f1 = true;
            Set<String> allCallers = new LinkedHashSet<>();
            if (sites != null) {
                fired++;
                for (Site st : sites) {
                    // BFS to vanilla callers
                    List<String> vc = new ArrayList<>();
                    Deque<String> dq = new ArrayDeque<>(); Set<String> seen = new HashSet<>();
                    String sk = st.cls + "." + st.method + st.desc;
                    if (st.kind.equals("patched-vanilla")) vc.add(sk); else { dq.add(sk); seen.add(sk); int depth = 0;
                        Map<String, Integer> dist = new HashMap<>(); dist.put(sk, 0);
                        while (!dq.isEmpty() && vc.size() < 12) {
                            String k = dq.poll(); int dd = dist.get(k); if (dd >= 4) continue;
                            Set<String> cs = callers.get(k); if (cs == null) continue;
                            for (String ck : cs) {
                                if (!seen.add(ck)) continue;
                                Ref rr = refs.get(ck); if (rr == null) continue;
                                if (rr.cls.startsWith("net/minecraft/") || !rr.cls.startsWith("net/minecraftforge/")) vc.add(ck);
                                else { dist.put(ck, dd + 1); dq.add(ck); }
                            }
                        }
                    }
                    o.append(f1 ? "" : ",").append("{\"class\":" + q(st.cls) + ",\"method\":" + q(st.method) + ",\"mcp\":" + q(st.mcp) + ",\"desc\":" + q(st.desc) + ",\"line\":" + st.line + ",\"file\":" + q(st.file)
                            + ",\"kind\":" + q(st.kind) + ",\"bus\":" + q(st.posts ? st.bus : st.bus + "?") + ",\"postsInSameMethod\":" + st.posts + ",\"vanillaCallers\":[");
                    boolean f2 = true;
                    for (String ck : vc) { Ref rr = refs.get(ck); o.append(f2 ? "" : ",").append("{\"class\":" + q(rr.cls) + ",\"method\":" + q(rr.method) + ",\"mcp\":" + q(rr.mcp) + ",\"desc\":" + q(rr.desc) + ",\"line\":" + rr.line + "}"); f2 = false; }
                    o.append("]}");
                    f1 = false;
                }
            }
            o.append("]}");
            sb.append(first ? "" : ",\n").append(o); first = false;
        }
        sb.append("\n]\n");
        Files.write(out.resolve("events.generated.json"), sb.toString().getBytes(StandardCharsets.UTF_8));
        summary.put("eventClasses", events.size()); summary.put("eventsWithConstructSite", fired); summary.put("eventsCancelable", canc); summary.put("eventsHasResult", res);
    }
}
