#!/usr/bin/env python3
"""side.py <ours.png> <proto.png> <out.png> [scale] [x y w h]
Puts the whole window of ours (left) and prototype (right) side by side, scaled."""
import sys
from PIL import Image
o_p, p_p, out_p = sys.argv[1], sys.argv[2], sys.argv[3]
s = float(sys.argv[4]) if len(sys.argv) > 4 else 0.5
crop = tuple(int(a) for a in sys.argv[5:9]) if len(sys.argv) > 8 else None
o = Image.open(o_p).convert("RGB"); p = Image.open(p_p).convert("RGB")
if crop: o, p = o.crop(crop), p.crop(crop)
sz = (int(o.width * s), int(o.height * s))
o, p = o.resize(sz, Image.LANCZOS), p.resize(sz, Image.LANCZOS)
c = Image.new("RGB", (o.width + p.width + 4, o.height), (0, 128, 0))
c.paste(o, (0, 0)); c.paste(p, (o.width + 4, 0))
c.save(out_p); print(f"{out_p} {c.size} left=ours right=prototype")
