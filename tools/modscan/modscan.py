#!/usr/bin/env python3
"""modscan: static inventory of a Minecraft 1.8.9 Forge mod jar. NEVER executes mod code.

Usage: python3 -I modscan.py JAR [--javap PATH] [--out FILE.json]

Reads the jar with zipfile, parses .class files with a built-in parser (constant
pool, annotations), and runs `javap -c -p` (disassembler only) for call-site counts.
Output: JSON inventory (see `scan()`).
"""
import sys, os, re, json, struct, zipfile, subprocess, tempfile, collections, argparse

DEFAULT_JAVAP = os.path.join(os.path.dirname(os.path.abspath(__file__)),
    "../../../../../.mc-tools/jdk8/jdk8/Contents/Home/bin/javap")
JAVAP_CANDIDATES = [
    "/Volumes/SSD/projects/rustmc/.mc-tools/jdk8/jdk8/Contents/Home/bin/javap",
    "/Volumes/SSD/projects/rustmc/.mc-tools/jdk8/Contents/Home/bin/javap",
]

# ---------------------------------------------------------------- class parser
class R:
    def __init__(s, b): s.b, s.o = b, 0
    def u1(s): v = s.b[s.o]; s.o += 1; return v
    def u2(s): v = struct.unpack_from(">H", s.b, s.o)[0]; s.o += 2; return v
    def u4(s): v = struct.unpack_from(">I", s.b, s.o)[0]; s.o += 4; return v
    def raw(s, n): v = s.b[s.o:s.o+n]; s.o += n; return v

def parse_class(data):
    r = R(data)
    if r.u4() != 0xCAFEBABE: raise ValueError("bad magic")
    minor, major = r.u2(), r.u2()
    n = r.u2(); cp = [None]*n; i = 1
    while i < n:
        t = r.u1()
        if t == 1:
            l = r.u2(); cp[i] = (1, r.raw(l).decode("utf-8", "replace"))
        elif t in (3, 4): cp[i] = (t, r.u4())
        elif t in (5, 6): cp[i] = (t, r.raw(8)); i += 1
        elif t in (7, 8, 16, 19, 20): cp[i] = (t, r.u2())
        elif t in (9, 10, 11, 12, 17, 18): cp[i] = (t, r.u2(), r.u2())
        elif t == 15: cp[i] = (t, r.u1(), r.u2())
        else: raise ValueError("cp tag %d" % t)
        i += 1
    def utf(i): return cp[i][1]
    def cls(i): return utf(cp[i][1])
    r.u2(); this = cls(r.u2()); sup = r.u2(); sup = cls(sup) if sup else None
    ifaces = [cls(r.u2()) for _ in range(r.u2())]

    def elem():
        tag = chr(r.u1())
        if tag in "BCDFIJSZs":
            i = r.u2()
            if tag == "s": return utf(i)
            return {"const": cp[i][1] if cp[i][0] != 5 else 0}
        if tag == "e": return {"enum": utf(r.u2()), "const": utf(r.u2())}
        if tag == "c": return {"class": utf(r.u2())}
        if tag == "@": return annot()
        if tag == "[": return [elem() for _ in range(r.u2())]
        raise ValueError(tag)
    def annot():
        ty = utf(r.u2()); d = {}
        for _ in range(r.u2()):
            k = utf(r.u2()); d[k] = elem()
        return {"type": ty, "values": d}
    def attrs():
        out = {}
        for _ in range(r.u2()):
            name = utf(r.u2()); ln = r.u4(); end = r.o + ln
            if name in ("RuntimeVisibleAnnotations", "RuntimeInvisibleAnnotations"):
                out.setdefault("annotations", []).extend(annot() for _ in range(r.u2()))
            r.o = end
        return out
    fields = []
    for _ in range(r.u2()):
        acc = r.u2(); nm = utf(r.u2()); ds = utf(r.u2()); a = attrs()
        fields.append({"name": nm, "desc": ds, "access": acc, **a})
    methods = []
    for _ in range(r.u2()):
        acc = r.u2(); nm = utf(r.u2()); ds = utf(r.u2()); a = attrs()
        methods.append({"name": nm, "desc": ds, "access": acc, **a})
    ca = attrs()
    # constant-pool refs
    refs = []
    for e in cp:
        if e and e[0] in (9, 10, 11):
            owner = cls(e[1]); nt = cp[e[2]]
            refs.append(({9: "field", 10: "method", 11: "imethod"}[e[0]], owner, utf(nt[1]), utf(nt[2])))
    classes = {utf(e[1]) for e in cp if e and e[0] == 7}
    strings = [utf(e[1]) for e in cp if e and e[0] == 8]
    return {"name": this, "super": sup, "interfaces": ifaces, "major": major,
            "fields": fields, "methods": methods, "annotations": ca.get("annotations", []),
            "refs": refs, "classes": classes, "strings": strings}

# ---------------------------------------------------------------- helpers
def pkg_group(owner):
    o = owner
    while o.startswith("["): o = o[1:]
    if o.startswith("L") and o.endswith(";"): o = o[1:-1]
    for p, g in (("net/minecraft/launchwrapper/", "launchwrapper"), ("net/minecraftforge/fml/", "forge-fml"), ("net/minecraftforge/", "forge"),
                 ("cpw/mods/fml/", "fml-legacy"), ("net/minecraft/", "minecraft"),
                 ("org/lwjgl/", "lwjgl"), ("org/spongepowered/", "mixin"),
                 ("org/objectweb/asm/", "asm"), ("com/mojang/", "mojang"),
                 ("org/apache/", "apache"), ("com/google/", "guava-gson"),
                 ("java/", "java"), ("javax/", "java"), ("sun/", "jdk-internal"),
                 ("io/netty/", "netty")):
        if o.startswith(p): return g
    return "other"

OBF_RE = re.compile(r"^(func_\d+_[a-zA-Z]+_?|field_\d+_[a-zA-Z]+_?|func_\d+|field_\d+)$")
def name_kind(n):
    if OBF_RE.match(n): return "srg"
    if re.fullmatch(r"[a-z]{1,3}", n) or re.fullmatch(r"[a-z]\w?", n): return "notch"
    return "mcp"

def ann_val(a, k):
    v = a["values"].get(k); return v

def clean_cls(x):
    if isinstance(x, dict) and "class" in x:
        c = x["class"]; return c[1:-1].replace("/", ".") if c.startswith("L") else c
    return x

def desc_event_param(desc):
    m = re.match(r"\(L([^;]+);\)", desc)
    return m.group(1).replace("/", ".") if m else None

URL_RE = re.compile(r"(https?://[^\s\"'<>)]+|wss?://[^\s\"'<>)]+|\b\d{1,3}(?:\.\d{1,3}){3}(?::\d+)?\b)")

# ---------------------------------------------------------------- javap
CALL_RE = re.compile(r"^\s+\d+:\s+(invoke\w+|getstatic|putstatic|getfield|putfield)\s+#\d+(?:,\s*\d+)?\s*//\s*(?:Method|InterfaceMethod|Field)\s+(.*)$")
LDC_RE = re.compile(r"^\s+\d+:\s+ldc\w*\s+#\d+\s*//\s*String (.*)$")
CLS_RE = re.compile(r"^(?:[a-z ]+ )?(?:class|interface|enum) (\S+)")
METH_RE = re.compile(r"^  \S.*\(.*\)")

def run_javap(javap, jar, names):
    out = []
    for i in range(0, len(names), 300):
        chunk = names[i:i+300]
        p = subprocess.run([javap, "-c", "-p", "-cp", jar] + chunk, capture_output=True, text=True, errors="replace")
        out.append(p.stdout)
    return "".join(out)

def parse_javap(text):
    calls = collections.Counter()      # (kind, owner, name, desc) -> count
    sites = collections.defaultdict(set)  # key -> classes
    refl = []                          # (class, method, strings near reflection call)
    cur = None; curm = None; recent = []
    for line in text.splitlines():
        m = CLS_RE.match(line)
        if m and not line.startswith(" "):
            cur = m.group(1); curm = None; recent = []; continue
        if METH_RE.match(line) and line.endswith(";") and ":" not in line.split("(")[0]:
            curm = line.strip(); recent = []; continue
        m = LDC_RE.match(line)
        if m:
            recent.append(m.group(1)); recent = recent[-6:]; continue
        m = CALL_RE.match(line)
        if m:
            op, tgt = m.groups()
            mm = re.match(r"([^.]+)\.([^:]+):(.*)$", tgt)
            if not mm: continue
            owner, name, desc = mm.groups()
            owner = owner.strip('"')
            key = (op, owner, name, desc)
            calls[key] += 1; sites[key].add(cur)
            if (name in ("getDeclaredField", "getDeclaredMethod", "getField", "getMethod", "setAccessible", "findField",
                         "findMethod", "getPrivateValue", "setPrivateValue", "forName", "getObfuscatedFieldName")
                    or "ReflectionHelper" in owner):
                refl.append({"in": cur, "call": "%s.%s" % (owner, name), "strings": list(recent)})
            recent = []
    return calls, sites, refl

# ---------------------------------------------------------------- scan
def scan(jar, javap):
    z = zipfile.ZipFile(jar)
    names = z.namelist()
    res = {"jar": os.path.basename(jar), "entries": len(names)}
    res["non_class_files"] = [n for n in names if not n.endswith(".class") and not n.endswith("/")][:200]
    lowname = os.path.basename(jar).lower()
    def rd(n):
        try: return z.read(n).decode("utf-8", "replace")
        except KeyError: return None
    # manifest
    mf = rd("META-INF/MANIFEST.MF") or ""
    mfd = {}
    for l in mf.replace("\r\n ", "").replace("\n ", "").splitlines():
        if ":" in l:
            k, v = l.split(":", 1); mfd[k.strip()] = v.strip()
    res["manifest"] = {k: v for k, v in mfd.items() if k in (
        "FMLCorePlugin", "TweakClass", "FMLCorePluginContainsFMLMod", "MixinConfigs", "ForceLoadAsMod", "FMLAT",
        "Main-Class", "Premain-Class", "Agent-Class", "TweakOrder", "ModSide", "Manifest-Version",
        "Created-By", "Built-By", "Implementation-Title", "Implementation-Version", "Class-Path",
        "MixinTokenProvider", "FMLAT", "Can-Redefine-Classes")}
    res["mcmod_info"] = None
    mi = rd("mcmod.info")
    if mi:
        try: res["mcmod_info"] = json.loads(mi, strict=False)
        except Exception: res["mcmod_info"] = mi[:2000]
    # mixin configs, access transformers
    mixin_cfg = {}
    for n in names:
        if n.endswith(".json") and ("mixin" in n.lower()):
            try: mixin_cfg[n] = json.loads(z.read(n).decode("utf-8", "replace"), strict=False)
            except Exception: pass
    res["mixin_configs"] = mixin_cfg
    res["access_transformers"] = {n: z.read(n).decode("utf-8", "replace")[:4000] for n in names if n.lower().endswith("_at.cfg") or n.lower().endswith("at.cfg") or n.startswith("META-INF/") and n.endswith("_at.cfg")}
    res["services"] = {n: rd(n) for n in names if n.startswith("META-INF/services/")}
    res["native_libs"] = [n for n in names if re.search(r"\.(dll|so|dylib|jnilib)$", n, re.I)]
    res["nested_jars"] = [n for n in names if n.endswith(".jar")]
    # parse classes
    cl = {}
    majors = collections.Counter()
    for n in names:
        if not n.endswith(".class") or n.startswith("META-INF/versions/") and False: continue
        try: c = parse_class(z.read(n))
        except Exception as e:
            continue
        cl[c["name"]] = c; majors[c["major"]] += 1
    res["class_count"] = len(cl)
    res["class_major_versions"] = dict(majors)
    res["java_max"] = {52: "8", 53: "9", 54: "10", 55: "11", 56: "12", 57: "13", 58: "14", 59: "15", 60: "16", 61: "17", 62: "18", 63: "19", 64: "20", 65: "21"}.get(max(majors) if majors else 0)
    res["multi_release"] = [n for n in names if n.startswith("META-INF/versions/")][:5]
    # package roots (to spot shaded libs)
    roots = collections.Counter()
    for k in cl:
        parts = k.split("/"); roots["/".join(parts[:3] if len(parts) > 3 else parts[:-1])] += 1
    res["package_roots"] = roots.most_common(25)
    # identity / loader type
    mods = []; evh = []; transformers = []; tweakers = []; coremods = []; mixins = []
    chans = []
    for k, c in cl.items():
        for a in c["annotations"]:
            if a["type"] in ("Lnet/minecraftforge/fml/common/Mod;", "Lcpw/mods/fml/common/Mod;"):
                mods.append({"class": k, **{x: a["values"].get(x) for x in ("modid", "name", "version", "acceptedMinecraftVersions", "clientSideOnly", "dependencies", "guiFactory", "acceptableRemoteVersions") if x in a["values"]}})
            if a["type"] == "Lorg/spongepowered/asm/mixin/Mixin;":
                v = a["values"]
                mixins.append({"class": k, "targets": [clean_cls(x) for x in v.get("value", [])] + list(v.get("targets", [])),
                               "priority": v.get("priority")})
            if a["type"].endswith("IFMLLoadingPlugin$TransformerExclusions;") or "IFMLLoadingPlugin" in a["type"]:
                coremods.append({"class": k, "ann": a["type"], "values": a["values"]})
        for m in c["methods"]:
            for a in m.get("annotations", []):
                t = a["type"]
                if t.endswith("/SubscribeEvent;"):
                    ev = desc_event_param(m["desc"])
                    evh.append({"class": k, "method": m["name"], "event": ev,
                                "priority": a["values"].get("priority", {}).get("const") if a["values"].get("priority") else None,
                                "receiveCanceled": a["values"].get("receiveCanceled")})
        ifs = set(c["interfaces"])
        if "net/minecraft/launchwrapper/IClassTransformer" in ifs: transformers.append(k)
        if "net/minecraft/launchwrapper/ITweaker" in ifs: tweakers.append(k)
        if "net/minecraftforge/fml/relauncher/IFMLLoadingPlugin" in ifs or "cpw/mods/fml/relauncher/IFMLLoadingPlugin" in ifs: coremods.append({"class": k, "iface": "IFMLLoadingPlugin"})
        if any(s.startswith("org/spongepowered/asm/mixin/transformer/") for s in ifs) or "org/spongepowered/asm/mixin/extensibility/IMixinConfigPlugin" in ifs:
            tweakers.append(k + " (mixin plugin)")
    res["mods"] = mods
    # what do ASM transformers / coremod plugins patch? (string constants of those classes)
    ts = {}
    for k in list(transformers) + [x["class"] for x in coremods]:
        if k in cl:
            ts[k] = sorted({x for x in cl[k]["strings"] if len(x) < 200})[:120]
    res["transformer_strings"] = ts
    res["event_handlers"] = evh
    res["class_transformers"] = transformers
    res["tweakers"] = tweakers
    res["coremods"] = coremods
    res["mixin_classes"] = mixins
    # mixin injections
    inj = []
    for k, c in cl.items():
        if not any(a["type"] == "Lorg/spongepowered/asm/mixin/Mixin;" for a in c["annotations"]): continue
        for m in c["methods"]:
            for a in m.get("annotations", []):
                if a["type"].startswith("Lorg/spongepowered/asm/mixin/"):
                    v = a["values"]
                    e = {"mixin": k, "kind": a["type"].rsplit("/", 1)[1].rstrip(";"), "mixin_method": m["name"]}
                    for key in ("method", "target", "value", "constant"):
                        if key in v: e[key] = v[key]
                    at = v.get("at")
                    if at:
                        ats = at if isinstance(at, list) else [at]
                        e["at"] = [{"value": x["values"].get("value"), "target": x["values"].get("target"), "ordinal": x["values"].get("ordinal")} for x in ats if isinstance(x, dict)]
                    inj.append(e)
        for f in c["fields"]:
            for a in f.get("annotations", []):
                if a["type"].endswith("/Shadow;"): inj.append({"mixin": k, "kind": "Shadow", "field": f["name"]})
    res["mixin_injections"] = inj
    # network channels & misc strings
    chan = set(); urls = set(); sus = set()
    for k, c in cl.items():
        for s in c["strings"]:
            if URL_RE.search(s) and len(s) < 300: urls.add((k, s))
        for r_ in c["refs"]:
            pass
        names_ = {r_[2] for r_ in c["refs"]}
        if names_ & {"registerNetworkChannel", "newSimpleChannel", "newEventDrivenChannel", "getNewSimpleChannel"} or \
           any(r_[1] in ("net/minecraftforge/fml/common/network/NetworkRegistry", "cpw/mods/fml/common/network/NetworkRegistry", "net/minecraftforge/fml/common/network/simpleimpl/SimpleNetworkWrapper") for r_ in c["refs"]) or \
           "net/minecraft/network/play/client/C17PacketCustomPayload" in c["classes"]:
            chan.add(k)
        for r_ in c["refs"]:
            if r_[1] in ("java/lang/Runtime", "java/lang/ProcessBuilder") or r_[1].startswith("java/net/") and r_[2] in ("openConnection", "<init>") and r_[1] in ("java/net/URL", "java/net/Socket", "java/net/HttpURLConnection", "java/net/URLConnection"):
                sus.add((k, "%s.%s" % (r_[1], r_[2])))
            if r_[1] in ("java/lang/System",) and r_[2] in ("loadLibrary", "load"): sus.add((k, "System.loadLibrary"))
            if r_[1] == "java/lang/Runtime" and r_[2] in ("load", "loadLibrary"): sus.add((k, "Runtime.load"))
            if r_[1] == "sun/misc/Unsafe": sus.add((k, "sun.misc.Unsafe"))
            if r_[1] == "java/lang/ClassLoader" and r_[2] == "defineClass": sus.add((k, "ClassLoader.defineClass"))
            if r_[1] == "java/lang/instrument/Instrumentation": sus.add((k, "Instrumentation"))
            if r_[1].startswith("java/awt/") and r_[1] in ("java/awt/Robot",): sus.add((k, "java.awt.Robot"))
    res["network_channel_classes"] = sorted(chan)
    res["channel_name_candidates"] = sorted({s for c in cl.values() for s in c["strings"] if re.fullmatch(r"[A-Za-z0-9_|:.\-]{2,20}", s) and "net/minecraft/network/play/client/C17PacketCustomPayload" in c["classes"] and not "/" in s})[:80]
    res["url_strings"] = sorted({"%s: %s" % (k.rsplit("/", 1)[-1], s) for k, s in urls})[:150]
    res["sensitive_api_use"] = sorted({"%s %s" % (k, w) for k, w in sus})[:150]
    # name-based identity (OptiFine policy)
    ident = []
    if "optifine" in lowname: ident.append("file name")
    if any(n.lower().startswith("optifine/") or "/optifine/" in n.lower() for n in names): ident.append("optifine package")
    if any("optifineclasstransformer" in n.lower() for n in names): ident.append("OptiFineClassTransformer")
    blob = (mf + (mi or "")).lower()
    if "optifine" in blob: ident.append("manifest/mcmod.info")
    res["optifine_identity"] = ident
    # references: constant-pool (per-class unique) and javap (call sites)
    ref_cls = collections.defaultdict(set)   # (group, kind, owner, name, desc) -> classes
    for k, c in cl.items():
        for kind, owner, name, desc in c["refs"]:
            if owner.startswith("["): continue
            ref_cls[(kind, owner, name, desc)].add(k)
    class_refs = collections.defaultdict(set)
    for k, c in cl.items():
        for cn in c["classes"]:
            class_refs[cn].add(k)
    calls = collections.Counter(); refl = []
    if javap and cl:
        txt = run_javap(javap, jar, [k.replace("/", ".") for k in cl])
        calls, sites, refl = parse_javap(txt)
    res["reflection"] = refl[:400]
    grp = collections.defaultdict(list)
    def add(kind, owner, name, desc, count, ncls):
        g = pkg_group(owner)
        if g in ("java", "other", "jdk-internal", "apache", "guava-gson", "netty"): return
        grp[g].append({"kind": kind, "owner": owner.replace("/", "."), "name": name, "desc": desc, "call_sites": count,
                       "classes": ncls, "namespace": name_kind(name) if g == "minecraft" else None})
    keys = set(ref_cls) | set(k[1:] for k in calls)
    seen = set()
    for (kind, owner, name, desc) in ref_cls:
        cnt = sum(v for k2, v in calls.items() if k2[1:] == (owner, name, desc)) if calls else 0
        add(kind, owner, name, desc, cnt, len(ref_cls[(kind, owner, name, desc)]))
    res["external_refs"] = {g: sorted(v, key=lambda e: (-e["call_sites"], -e["classes"], e["owner"], e["name"])) for g, v in grp.items()}
    cr = collections.defaultdict(list)
    for cn, users in class_refs.items():
        g = pkg_group(cn)
        if g in ("minecraft", "forge", "forge-fml", "fml-legacy", "lwjgl", "mixin", "asm", "mojang"):
            cr[g].append({"class": cn.replace("/", "."), "classes": len(users)})
    res["external_classes"] = {g: sorted(v, key=lambda e: -e["classes"]) for g, v in cr.items()}
    ns = collections.Counter()
    for e in res["external_refs"].get("minecraft", []):
        ns[e["namespace"]] += 1
    res["minecraft_name_namespaces"] = dict(ns)
    # features: gui hints
    return res

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("jar"); ap.add_argument("--javap"); ap.add_argument("--out")
    a = ap.parse_args()
    jp = a.javap or next((p for p in JAVAP_CANDIDATES if os.path.exists(p)), None)
    r = scan(a.jar, jp)
    s = json.dumps(r, indent=1, default=lambda o: sorted(o) if isinstance(o, set) else str(o))
    if a.out: open(a.out, "w").write(s)
    else: print(s)

if __name__ == "__main__":
    main()
