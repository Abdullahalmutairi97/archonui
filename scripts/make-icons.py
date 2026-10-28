#!/usr/bin/env python3
"""Builds ArchonUI's app icons from the mark art.

Source: assets/logo/archonui-logo.png (the 1024px mark: a white blade with a red glow, on transparency).
Writes:  assets/icons/hicolor/<N>x<N>/apps/archonui.png (16..512) and assets/logo/archonui-icon.png (1024).

Each size is drawn on its own rather than scaled down from one big icon:
- 16, 24 and 32 use a bolder blade and a thin red rim instead of the soft glow, so the mark stays a
  readable shape instead of a grey smudge;
- 48 and up keep the art's glow, and 128 and up add a soft ember behind the mark and a drop shadow.

Run: uv run --with pillow --with numpy scripts/make-icons.py
"""
from __future__ import annotations

import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageChops, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "assets/logo/archonui-logo.png"
SIZES = [16, 24, 32, 48, 64, 128, 256, 512]
EMBER = (226, 38, 44)


def load_mark() -> tuple[np.ndarray, np.ndarray]:
    """The mark split into (blade alpha, glow alpha), both 0..1 floats, cropped to the mark."""
    im = Image.open(SRC).convert("RGBA")
    im = im.crop(im.getchannel("A").point(lambda v: 255 if v > 8 else 0).getbbox())
    a = np.asarray(im, dtype=np.float32) / 255.0
    r, g, b, alpha = a[..., 0], a[..., 1], a[..., 2], a[..., 3]
    base = np.maximum(g, b)
    redness = np.clip(r - base, 0, 1)
    # Blade: bright and grey. Glow: red beyond the blade.
    blade = np.clip(alpha * np.clip((base - 0.35) / 0.35, 0, 1), 0, 1)
    glow = np.clip(alpha * np.clip(redness / 0.5, 0, 1), 0, 1)
    return blade, glow


def to_img(arr: np.ndarray) -> Image.Image:
    return Image.fromarray(np.clip(arr * 255 + 0.5, 0, 255).astype(np.uint8), "L")


def rounded_mask(size: int, box: tuple[float, float, float, float], radius: float, ss: int = 4) -> Image.Image:
    big = Image.new("L", (size * ss, size * ss), 0)
    x0, y0, x1, y1 = (v * ss for v in box)
    ImageDraw.Draw(big).rounded_rectangle((x0, y0, x1 - 1, y1 - 1), radius=radius * ss, fill=255)
    return big.resize((size, size), Image.LANCZOS)


def vertical_gradient(size: int, top: tuple[int, int, int], bottom: tuple[int, int, int]) -> Image.Image:
    t = np.linspace(0, 1, size, dtype=np.float32)[:, None, None]
    c = np.array(top, np.float32) * (1 - t) + np.array(bottom, np.float32) * t
    return Image.fromarray(np.repeat(c, size, axis=1).astype(np.uint8), "RGB")


def radial(size: int, cx: float, cy: float, rx: float, ry: float) -> np.ndarray:
    y, x = np.mgrid[0:size, 0:size].astype(np.float32)
    d = np.sqrt(((x - cx) / rx) ** 2 + ((y - cy) / ry) ** 2)
    return np.clip(1 - d, 0, 1) ** 2


def tile(size: int, small: bool) -> tuple[Image.Image, Image.Image, tuple[float, float, float, float]]:
    """The dark rounded tile: (rgba, mask, box). Small sizes fill the canvas; larger ones leave room
    for a drop shadow, but much less than before (3% a side instead of 6%)."""
    inset = 0 if small else round(size * 0.03)
    box = (inset, inset, size - inset, size - inset)
    side = size - 2 * inset
    radius = side * (0.20 if small else 0.225)
    mask = rounded_mask(size, box, radius)
    body = vertical_gradient(size, (30, 30, 36), (8, 8, 10)).convert("RGBA")
    if not small:
        # A faint ember under the mark's lower half, so the tile isn't flat black.
        ember = radial(size, size / 2, size * 0.62, side * 0.55, side * 0.48) * 0.22
        layer = Image.new("RGBA", (size, size), EMBER + (0,))
        layer.putalpha(to_img(ember))
        body = Image.alpha_composite(body, layer)
        # Rim: a light top edge that fades down, and a dark outer line so the tile holds on dark panels.
        rim_w = max(1.0, side / 128)
        inner = rounded_mask(size, (box[0] + rim_w, box[1] + rim_w, box[2] - rim_w, box[3] - rim_w), radius - rim_w)
        ring = ImageChops.subtract(mask, inner)
        fade = np.linspace(0.16, 0.0, size, dtype=np.float32)[:, None] * np.ones((1, size), np.float32)
        hi = Image.new("RGBA", (size, size), (255, 255, 255, 0))
        hi.putalpha(ImageChops.multiply(ring, to_img(fade)))
        body = Image.alpha_composite(body, hi)
    else:
        # One soft lighter pixel row along the top keeps the tile from reading as a hole.
        ring = ImageChops.subtract(mask, rounded_mask(size, (box[0], box[1] + 1, box[2], box[3]), radius))
        hi = Image.new("RGBA", (size, size), (255, 255, 255, 0))
        hi.putalpha(ring.point(lambda v: v * 0.12))
        body = Image.alpha_composite(body, hi)
    out = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    out.paste(body, (0, 0), mask)
    return out, mask, box


def place(arr: np.ndarray, size: int, height: float, cy: float) -> Image.Image:
    """`arr` scaled so its height is `height` px and centred at (size/2, cy), on a size×size canvas."""
    h, w = arr.shape
    scale = height / h
    nw, nh = max(1, round(w * scale)), max(1, round(h * scale))
    src = to_img(arr).resize((nw, nh), Image.LANCZOS)
    out = Image.new("L", (size, size), 0)
    out.paste(src, (round(size / 2 - nw / 2), round(cy - nh / 2)))
    return out


def dilate(arr: np.ndarray, px: int) -> np.ndarray:
    if px <= 0:
        return arr
    img = to_img(arr).filter(ImageFilter.MaxFilter(px * 2 + 1))
    return np.asarray(img, np.float32) / 255.0


def solid(size: int, rgb: tuple[int, int, int], alpha: Image.Image) -> Image.Image:
    layer = Image.new("RGBA", (size, size), rgb + (0,))
    layer.putalpha(alpha)
    return layer


def icon(size: int, blade: np.ndarray, glow: np.ndarray) -> Image.Image:
    small = size <= 32
    body, mask, box = tile(size, small)
    side = box[2] - box[0]
    # The mark is now ~86% of the tile's height (it was ~81% of a smaller tile).
    mark_h = side * (0.90 if small else 0.86)
    cy = (box[1] + box[3]) / 2
    src_h = blade.shape[0]
    if small:
        # Bold the blade so its strokes are at least ~1px at this size, then add a 1px-ish red rim
        # around it in place of the soft glow.
        per_px = src_h / mark_h
        bold = dilate(blade, round(per_px * {16: 0.22, 24: 0.20, 32: 0.18}[size]))
        rim = dilate(bold, round(per_px * 0.40))
        b = place(bold, size, mark_h, cy)
        r = place(rim, size, mark_h, cy)
        b = b.filter(ImageFilter.UnsharpMask(radius=0.6, percent=120, threshold=0))
        out = Image.alpha_composite(body, solid(size, (200, 30, 36), ImageChops.multiply(r.point(lambda v: v * 0.8), mask)))
        out = Image.alpha_composite(out, solid(size, (246, 246, 248), ImageChops.multiply(b, mask)))
        return out
    g = place(glow, size, mark_h, cy)
    b = place(blade, size, mark_h, cy)
    out = body
    if size >= 128:
        # A wider, softer ember around the whole mark: the glow carried a little further.
        halo = place(np.maximum(glow, blade), size, mark_h, cy).filter(ImageFilter.GaussianBlur(size / 40))
        out = Image.alpha_composite(out, solid(size, EMBER, ImageChops.multiply(halo.point(lambda v: v * 0.55), mask)))
    out = Image.alpha_composite(out, solid(size, (232, 36, 42), ImageChops.multiply(g, mask)))
    # The blade: near-white with a slight cool-to-warm shade from top to bottom.
    shade = vertical_gradient(size, (250, 250, 252), (214, 212, 214)).convert("RGBA")
    shade.putalpha(ImageChops.multiply(b, mask))
    out = Image.alpha_composite(out, shade)
    if size <= 64:
        out = out.filter(ImageFilter.UnsharpMask(radius=0.8, percent=60, threshold=0))
    if size >= 64:
        out = with_shadow(out, mask, size)
    return out


def with_shadow(img: Image.Image, mask: Image.Image, size: int) -> Image.Image:
    shadow = mask.filter(ImageFilter.GaussianBlur(size / 64)).point(lambda v: v * 0.45)
    shifted = Image.new("L", (size, size), 0)
    shifted.paste(shadow, (0, max(1, round(size / 128))))
    base = solid(size, (0, 0, 0), shifted)
    return Image.alpha_composite(base, img)


def main() -> int:
    blade, glow = load_mark()
    for size in SIZES:
        out = ROOT / f"assets/icons/hicolor/{size}x{size}/apps/archonui.png"
        out.parent.mkdir(parents=True, exist_ok=True)
        icon(size, blade, glow).save(out, optimize=True)
        print(out.relative_to(ROOT))
    big = ROOT / "assets/logo/archonui-icon.png"
    icon(1024, blade, glow).save(big, optimize=True)
    print(big.relative_to(ROOT))
    return 0


if __name__ == "__main__":
    sys.exit(main())
