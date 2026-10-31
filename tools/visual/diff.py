#!/usr/bin/env python3
"""Compare shots/vanilla/screenshots/<scene>.png with shots/rustmc/<scene>.png.
Prints mean abs error (0..255 per channel) and the % of pixels whose max channel diff exceeds
THRESH; writes shots/diff/<scene>.png (heatmap) and shots/diff/<scene>_side.png
(vanilla | rustmc | heat, half size).  Usage: diff.py [filter] [--md]"""
import sys, os, argparse
import numpy as np
from PIL import Image

THRESH = 24
root = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..")
shots = os.environ.get("VISUAL_SHOTS", os.path.join(root, "shots"))
va, ru, out = [os.path.join(shots, d) for d in ("vanilla/screenshots", "rustmc", "diff")]
os.makedirs(out, exist_ok=True)
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("filter", nargs="?", default="")
parser.add_argument("--md", action="store_true")
parser.add_argument("--world", action="append", choices=("title", "grid", "terrain"))
args = parser.parse_args()
rows = []
failed = False
with open(os.path.join(root, "tools/visual/scenes.txt")) as scene_file:
    scenes = [line.split() for line in scene_file if line.strip() and not line.lstrip().startswith("#")]
for scene in scenes:
    n, world = scene[:2]
    if args.filter not in n or (args.world and world not in args.world):
        continue
    f = n + ".png"
    if not os.path.exists(os.path.join(va, f)) or not os.path.exists(os.path.join(ru, f)):
        rows.append((n, None, None))
        failed = True
        continue
    a = np.asarray(Image.open(os.path.join(va, f)).convert("RGB")).astype(np.int16)
    img = Image.open(os.path.join(ru, f)).convert("RGB")
    if img.size != (a.shape[1], a.shape[0]):
        print(f"{n}: size mismatch: vanilla {(a.shape[1], a.shape[0])}, rustmc {img.size}", file=sys.stderr)
        failed = True
        continue
    b = np.asarray(img).astype(np.int16)
    d = np.abs(a - b)
    mae = d.mean()
    pct = (d.max(axis=2) > THRESH).mean() * 100
    h = np.clip(d.max(axis=2) * 3, 0, 255).astype(np.uint8)
    heat = np.stack([h, h // 3, np.zeros_like(h)], axis=2)
    Image.fromarray(heat).save(os.path.join(out, f))
    side = np.concatenate([a.astype(np.uint8), b.astype(np.uint8), heat], axis=1)
    Image.fromarray(side).resize((side.shape[1] // 2, side.shape[0] // 2)).save(os.path.join(out, n + "_side.png"))
    rows.append((n, mae, pct))
if args.md:
    print("| scene | MAE | %% px > %d |\n|---|---|---|" % THRESH)
else:
    print("%-18s %8s %9s" % ("scene", "MAE", "%>" + str(THRESH)))
for n, m, p in rows:
    if m is None:
        print(("| %s | missing | |" % n) if args.md else "%-18s missing" % n)
    else:
        print(("| %s | %.2f | %.1f%% |" % (n, m, p)) if args.md else "%-18s %8.2f %8.1f%%" % (n, m, p))
if rows and not args.md:
    ok = [m for _, m, _ in rows if m is not None]
    print("%-18s %8.2f" % ("MEAN", sum(ok) / max(len(ok), 1)))

if not rows or failed:
    sys.exit(1)
