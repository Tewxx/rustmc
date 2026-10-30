#!/usr/bin/env python3
"""report: merge modscan JSON files into ranked Markdown tables (no mod code is run).

Usage: python3 -I report.py SCAN1.json SCAN2.json ... [--top N]
"""
import sys, json, collections

def main():
    top = 25
    args = sys.argv[1:]
    if "--top" in args:
        i = args.index("--top"); top = int(args[i+1]); del args[i:i+2]
    scans = [json.load(open(a)) for a in args]
    names = [s["jar"] for s in scans]
    def table(title, rows, cols):
        print("\n### %s\n" % title)
        print("| " + " | ".join(cols) + " |"); print("|" + "---|"*len(cols))
        for r in rows: print("| " + " | ".join(str(x) for x in r) + " |")
    # minecraft members: mods referencing, total call sites
    for grp, label in (("minecraft", "net.minecraft members"), ("forge", "net.minecraftforge members"),
                       ("fml-legacy", "cpw.mods.fml members"), ("forge-fml", "net.minecraftforge.fml members"),
                       ("lwjgl", "org.lwjgl members")):
        agg = collections.defaultdict(lambda: [set(), 0, None])
        for s in scans:
            for e in s["external_refs"].get(grp, []):
                key = (e["owner"], e["name"], e["desc"])
                a = agg[key]; a[0].add(s["jar"]); a[1] += e["call_sites"]; a[2] = e.get("namespace")
        rows = sorted(agg.items(), key=lambda kv: (-len(kv[1][0]), -kv[1][1]))[:top]
        table("Top " + label + " (by #mods, then call sites)",
              [("`%s.%s`" % (k[0], k[1]), "`%s`" % k[2][:60], len(v[0]), v[1], v[2] or "") for k, v in rows],
              ["member", "desc", "mods", "call sites", "naming"])
    # classes
    for grp, label in (("minecraft", "net.minecraft classes"), ("forge", "Forge classes"), ("fml-legacy", "cpw.mods.fml classes"),
                       ("forge-fml", "net.minecraftforge.fml classes")):
        agg = collections.defaultdict(lambda: [set(), 0])
        for s in scans:
            for e in s["external_classes"].get(grp, []):
                a = agg[e["class"]]; a[0].add(s["jar"]); a[1] += e["classes"]
        rows = sorted(agg.items(), key=lambda kv: (-len(kv[1][0]), -kv[1][1]))[:top]
        table("Top " + label, [("`%s`" % k, len(v[0]), v[1]) for k, v in rows], ["class", "mods", "referencing classes"])
    ev = collections.defaultdict(lambda: [set(), 0])
    for s in scans:
        for e in s["event_handlers"]:
            if e["event"]:
                a = ev[e["event"]]; a[0].add(s["jar"]); a[1] += 1
    rows = sorted(ev.items(), key=lambda kv: (-len(kv[1][0]), -kv[1][1]))
    table("@SubscribeEvent event classes", [("`%s`" % k, len(v[0]), v[1]) for k, v in rows], ["event", "mods", "handlers"])
    tg = collections.defaultdict(lambda: [set(), 0])
    for s in scans:
        for m in s["mixin_classes"]:
            for t in m["targets"]:
                a = tg[t]; a[0].add(s["jar"]); a[1] += 1
    rows = sorted(tg.items(), key=lambda kv: (-len(kv[1][0]), -kv[1][1]))
    table("Mixin target classes", [("`%s`" % k, ", ".join(sorted(v[0])), v[1]) for k, v in rows], ["target", "mods", "mixins"])
    inj = collections.defaultdict(lambda: [set(), []])
    for s in scans:
        tgt = {m["class"]: (m["targets"] or ["?"])[0] for m in s["mixin_classes"]}
        for i in s["mixin_injections"]:
            if "method" in i:
                for m in (i["method"] if isinstance(i["method"], list) else [i["method"]]):
                    k = "%s#%s" % (tgt.get(i["mixin"], "?"), m)
                    a = inj[k]; a[0].add(s["jar"])
                    at = i.get("at")
                    a[1].append("%s%s" % (i["kind"], ("@" + "/".join(str(x["value"]) + (":" + x["target"] if x.get("target") else "") for x in at)) if at else ""))
    rows = sorted(inj.items(), key=lambda kv: (-len(kv[1][0]), kv[0]))
    table("Mixin injection points", [("`%s`" % k, len(v[0]), "; ".join(sorted(set(v[1])))[:200]) for k, v in rows], ["target method", "mods", "kind@at"])
    gl = collections.Counter()
    for s in scans:
        for e in s["external_refs"].get("lwjgl", []):
            gl[(e["owner"].replace("org.lwjgl.", ""), e["name"])] += e["call_sites"]
    print("\n### LWJGL entry points by call sites\n")
    for (o, n), c in gl.most_common(top*3): print("- `%s.%s` x%d" % (o, n, c))

if __name__ == "__main__":
    main()
