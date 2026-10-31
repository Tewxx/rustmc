#!/usr/bin/env python3
"""Index the Forge hook call sites that Forge's patches add to vanilla classes.

Reads <forge-src>/patches/minecraft/**.patch and records, per patched vanilla class, every added line that
references net.minecraftforge.* (patch file, line in the .patch, hunk header, called symbols). Only symbol
names and positions are recorded, never the patch text itself.

usage: hookindex.py <forge-src> <out.json>
"""
import json, os, re, sys

src, out = sys.argv[1], sys.argv[2]
root = os.path.join(src, "patches", "minecraft")
call_re = re.compile(r"net\.minecraftforge\.((?:[A-Za-z_][\w]*\.)+)([A-Za-z_]\w*)\s*(\(|\b)")
new_re = re.compile(r"new\s+net\.minecraftforge\.([\w.]+)\s*\(")
res = {}
for dp, _, fns in os.walk(root):
    for fn in sorted(fns):
        if not fn.endswith(".patch"):
            continue
        p = os.path.join(dp, fn)
        rel = os.path.relpath(p, root)
        cls = rel[: -len(".java.patch")]
        hunk, items = "", []
        with open(p, encoding="utf-8", errors="replace") as f:
            for n, line in enumerate(f, 1):
                if line.startswith("@@"):
                    hunk = line.strip()
                    continue
                if not line.startswith("+") or line.startswith("+++"):
                    continue
                syms = []
                for m in new_re.finditer(line):
                    syms.append("new " + m.group(1))
                for m in call_re.finditer(line):
                    if m.group(3) == "(":
                        owner = m.group(1).rstrip(".").split(".")[-1]
                        syms.append(owner + "." + m.group(2))
                    elif m.group(1).rstrip(".").split(".")[-1] in ("MinecraftForge",) and m.group(2).endswith("BUS"):
                        syms.append("MinecraftForge." + m.group(2))
                if syms:
                    items.append({"patchLine": n, "hunk": hunk, "symbols": sorted(set(syms))})
        res[cls] = {"patch": "patches/minecraft/" + rel, "hooks": items}
with open(out, "w") as f:
    json.dump(res, f, indent=1, sort_keys=True)
print(len(res), "patched vanilla classes with forge hooks;", sum(len(v["hooks"]) for v in res.values()), "hook lines")
