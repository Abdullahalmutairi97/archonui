import sys
from PIL import Image

img = Image.open(sys.argv[1]).convert('RGB')
mode = sys.argv[2]  # 'row' or 'col'
fixed = int(sys.argv[3])
lo = int(sys.argv[4]); hi = int(sys.argv[5])
pix = img.load()
prev = None
start = lo
def cc(p):
    return p
runs = []
for i in range(lo, hi):
    p = pix[i, fixed] if mode == 'row' else pix[fixed, i]
    if p is None:
        continue
    if prev is None:
        prev = p; start = i
    elif max(abs(a-b) for a, b in zip(p, prev)) > 8:
        runs.append((start, i-1, prev))
        prev = p; start = i
runs.append((start, hi-1, prev))
for a, b, p in runs:
    print(f"{a}-{b} ({b-a+1}px) #{p[0]:02x}{p[1]:02x}{p[2]:02x}")
