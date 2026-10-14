#!/usr/bin/env python3
"""pair.py <ours.png> <proto.png> <out.png> <x> <y> <w> <h> [zoom]
Both screenshots are 1920x1080 at 1 design px == 1 px, so the same rect is cropped from each.
Stacks OURS on top, PROTOTYPE below, separated by a green line."""
import sys
from PIL import Image

o_p, p_p, out_p = sys.argv[1], sys.argv[2], sys.argv[3]
x, y, w, h = (int(a) for a in sys.argv[4:8])
zoom = float(sys.argv[8]) if len(sys.argv) > 8 else 1.0
box = (x, y, x + w, y + h)
o = Image.open(o_p).convert("RGB").crop(box)
p = Image.open(p_p).convert("RGB").crop(box)
if zoom != 1.0:
    sz = (int(w * zoom), int(h * zoom))
    o = o.resize(sz, Image.NEAREST)
    p = p.resize(sz, Image.NEAREST)
c = Image.new("RGB", (o.width, o.height * 2 + 4), (0, 128, 0))
c.paste(o, (0, 0))
c.paste(p, (0, o.height + 4))
c.save(out_p)
print(f"{out_p} {c.size} (top=ours, bottom=prototype) rect={box} zoom={zoom}")
