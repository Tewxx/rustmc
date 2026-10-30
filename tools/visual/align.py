#!/usr/bin/env python3
"""Best integer (dx, dy) shift of rustmc vs vanilla for a scene region: a camera-mismatch probe.
Usage: align.py scene x0 y0 x1 y1"""
import sys, os
import numpy as np
from PIL import Image
root = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..")
n, x0, y0, x1, y1 = sys.argv[1], *map(int, sys.argv[2:6])
a = np.asarray(Image.open(f"{root}/shots/vanilla/screenshots/{n}.png").convert("L")).astype(float)
b = np.asarray(Image.open(f"{root}/shots/rustmc/{n}.png").convert("L")).astype(float)
best = None
for dy in range(-20, 21):
    for dx in range(-20, 21):
        e = np.abs(a[y0:y1, x0:x1] - b[y0 + dy:y1 + dy, x0 + dx:x1 + dx]).mean()
        if best is None or e < best[0]:
            best = (e, dx, dy)
print("best mae %.2f at rustmc offset dx=%d dy=%d (zero-shift mae %.2f)" % (best[0], best[1], best[2], np.abs(a[y0:y1, x0:x1] - b[y0:y1, x0:x1]).mean()))
