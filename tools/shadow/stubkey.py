#!/usr/bin/env python3
"""Print stub keys for crates/mc-forge/src/impls.rs: stubkey.py CLASS_SUBSTRING [MCP_METHOD_SUBSTRING]
e.g. stubkey.py MathHelper floor  ->  net/minecraft/util/MathHelper.func_76128_c  floor_double (D)I"""
import csv, os, sys
conf = os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../MCP-919/conf")
names = {r[0]: r[1] for r in csv.reader(open(os.path.join(conf, "methods.csv"))) if r}
cls, want = sys.argv[1], (sys.argv[2] if len(sys.argv) > 2 else "").lower()
for line in open(os.path.join(conf, "joined.srg")):
    p = line.split()
    if p[0] != "MD:": continue
    owner, _, srg = p[3].rpartition("/")
    if cls in owner and want in names.get(srg, srg).lower():
        print(f"{owner}.{srg}  {names.get(srg, '-')} {p[4]}")
