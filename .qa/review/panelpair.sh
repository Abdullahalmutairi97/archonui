#!/bin/bash
# panelpair.sh <tab>  -> /tmp/panel-<tab>.png (ours left, prototype right, panel region only)
cd /home/archonminipc/ArchonUI/.qa/review
python3 - "$1" <<'PY'
import sys
from PIL import Image
tab = sys.argv[1]
o = Image.open(f"main-10-tab-{tab}.png").convert("RGB").crop((1426, 0, 1920, 1080))
p = Image.open(f"proto1x-tab-{tab}.png").convert("RGB").crop((1426, 0, 1920, 1080))
c = Image.new("RGB", (o.width*2+4, o.height), (0,128,0))
c.paste(o,(0,0)); c.paste(p,(o.width+4,0))
c.save(f"/tmp/panel-{tab}.png"); print(f"/tmp/panel-{tab}.png {c.size}")
PY
