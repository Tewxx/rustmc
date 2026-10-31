#!/usr/bin/env python3
"""keep-real regexes for classes that mods' mixins need real bytecode in: mixin-keep.py SCAN.json...
A mixin that injects at HEAD/RETURN/TAIL (or only adds accessors) works on a stub; INVOKE/FIELD/NEW/CONSTANT points,
@Redirect, @ModifyArg/Variable/Constant and @Overwrite need the vanilla body. Classes the Rust side implements
(owners named in crates/mc-forge/src/impls) are skipped with a warning: they need per-method forcing in stub-methods.txt."""
import json, os, re, sys
here = os.path.dirname(os.path.abspath(__file__))
rust_owners = set()
impls = os.path.join(here, "../../crates/mc-forge/src/impls")
for f in os.listdir(impls):
    rust_owners |= set(re.findall(r'"((?:net|org)/[A-Za-z0-9_/$]+)\.', open(os.path.join(impls, f)).read()))
forced_owners = {l.split(".")[0] for l in open(os.path.join(here, "stub-methods.txt")) if l.strip() and not l.startswith("#")}
simple_at = {"HEAD", "RETURN", "TAIL"}
need = {}
for scan in sys.argv[1:]:
    d = json.load(open(scan))
    targets = {m["class"]: m["targets"] for m in d["mixin_classes"]}
    for inj in d["mixin_injections"]:
        ats = {a["value"] for a in inj.get("at") or []}
        if inj["kind"] in ("Inject",) and ats <= simple_at: continue
        if inj["kind"] in ("Accessor", "Invoker", "Shadow"): continue
        for t in targets.get(inj["mixin"], []): need.setdefault(t.replace(".", "/"), set()).add(f'{inj["kind"]} {sorted(ats)}')
for cls, why in sorted(need.items()):
    if cls in rust_owners and cls not in forced_owners:
        print(f"# SKIPPED {cls}: implemented in Rust ({'; '.join(sorted(why))}); list its Rust-handled methods in stub-methods.txt, then it can be real", file=sys.stderr)
        continue
    print("^" + cls + r"(\$[^/]*)?\.class$")
