//! Wallpapers and glass. GPUI has no backdrop blur, so the blurred wallpaper is pre-computed off the
//! UI thread and cropped to each glass surface, aligned with the full-canvas wallpaper behind it.
//! Specs: docs/design/layout.md §1.3 and §5, docs/design/themes.md §4, docs/UI_PORT_PLAN.md.
//!
//! Units: geometry and blur radii are design px on the 1920 × 1080 canvas. The shell scales that
//! canvas as a whole, so a blur of `r` design px is a Gaussian of `r × image width / 1920` image px.

use std::{
    collections::HashMap,
    panic::Location,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError},
};

use gpui::{
    div, fill, img, prelude::*, transparent_black, App, Bounds, Corners, Element, ElementId,
    GlobalElementId, Hsla, InspectorElementId, IntoElement, LayoutId, ObjectFit, Pixels,
    RenderImage, Size, Style, Window,
};
use crate::ui::scale::px;
use image::{imageops::FilterType, Frame, RgbaImage};
use smallvec::smallvec;

use crate::ui::{anim, theme::Wallpaper};

/// Wider wallpapers are scaled down before blurring: 2560 × 1440 × 4 bytes ≈ 14.7 MB.
const MAX_WIDTH: u32 = 2560;
/// Width of the design canvas that blur radii are measured against.
pub const DESIGN_WIDTH: f32 = 1920.0;
/// How far the wallpaper layer extends past each canvas edge, so blurred edges never show (layout.md §1.3).
pub const WALLPAPER_BLEED: f32 = 60.0;
/// Decoded sources kept in memory. Each is up to 14.7 MB.
const DECODED_CAP: usize = 2;
/// Ready variants kept in memory. Each is up to 14.7 MB.
const IMAGE_CAP: usize = 8;
/// Glass crops kept in memory. A crop is at most one surface at device resolution.
const CROP_CAP: usize = 32;

/// Wallpapers embedded at build time, by file name: the ArchonUI theme's glow. The backgrounds folder
/// holds the user's.
const EMBEDDED_WALLPAPERS: [(&str, &[u8]); 1] =
    [("archonui-glow.jpg", include_bytes!("../../assets/wallpapers/archonui-glow.jpg"))];

pub fn wallpaper_bytes(name: &str) -> Option<&'static [u8]> {
    EMBEDDED_WALLPAPERS.iter().find(|(n, _)| *n == name).map(|(_, bytes)| *bytes)
}

/// A theme's logo: the embedded ArchonUI mark by name, or a logo file by path. Files are read once.
pub fn logo_bytes(name: &str) -> Option<Arc<[u8]>> {
    static FILES: OnceLock<Mutex<HashMap<String, Option<Arc<[u8]>>>>> = OnceLock::new();
    // `archonui-mark.png#rrggbb`: the mark with its red glow in that colour, and `#rrggbb#rrggbb` also
    // with its blade in the second colour. Made once per name.
    if let Some((base, hexes)) = name.split_once('#') {
        let parse = |hex: &str| u32::from_str_radix(hex, 16).ok().filter(|_| hex.len() == 6);
        let (tint, blade) = match hexes.split_once('#') {
            Some((tint, blade)) => (parse(tint)?, Some(parse(blade)?)),
            None => (parse(hexes)?, None),
        };
        let plain = logo_bytes(base)?;
        let mut files = FILES.get_or_init(Default::default).lock().ok()?;
        return files.entry(name.to_string()).or_insert_with(|| tint_logo(&plain, tint, blade).map(Arc::from)).clone();
    }
    match name {
        "archonui-mark.png" => Some(Arc::from(include_bytes!("../../assets/logo/archonui-mark.png").as_slice())),
        path if Path::new(path).is_absolute() => {
            let mut files = FILES.get_or_init(Default::default).lock().ok()?;
            files.entry(path.to_string()).or_insert_with(|| std::fs::read(path).ok().map(Arc::from)).clone()
        }
        _ => None,
    }
}

/// The logo PNG with its red parts repainted in `tint` (`0xrrggbb`): each pixel's red beyond its green
/// and blue is the glow, and that much of the tint replaces it. The white blade stays white, or with
/// `blade` takes that colour, its shading kept as darker shades of it.
pub fn tint_logo(png: &[u8], tint: u32, blade: Option<u32>) -> Option<Vec<u8>> {
    let mut img = image::load_from_memory(png).ok()?.to_rgba8();
    let rgb = |c: u32| [(c >> 16) as u8 as f32, (c >> 8) as u8 as f32, c as u8 as f32];
    let [tr, tg, tb] = rgb(tint);
    let peak = tr.max(tg).max(tb).max(1.);
    let blade = blade.map(rgb);
    for p in img.pixels_mut() {
        let [r, g, b, a] = p.0;
        let base = g.max(b);
        let glow = f32::from(r.saturating_sub(base));
        if glow <= 0. && blade.is_none() {
            continue;
        }
        let body = |c: u8, i: usize| {
            let c = f32::from(c.min(base));
            blade.map_or(c, |bl| bl[i] * c / 255.)
        };
        let mix = |c: u8, t: f32, i: usize| (body(c, i) + glow * t / peak).round().clamp(0., 255.) as u8;
        p.0 = [mix(r, tr, 0), mix(g, tg, 1), mix(b, tb, 2), a];
    }
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).ok()?;
    Some(out)
}

/// The logo `name` (see `logo_bytes`) scaled so its longer side is `edge` pixels, as a GPUI image.
/// GPUI draws an image by sampling its full-size texture without mipmaps, so a 512px mark drawn at
/// 28px came out jagged and shimmering; scaling it here first, with Lanczos (or nearest-neighbour for
/// pixel-art logos), keeps every size crisp. Callers ask for twice the drawn size so HiDPI stays sharp.
/// A logo smaller than `edge` is used as it is. Made once per name, size and filter.
pub fn logo_image(name: &str, edge: u32, pixelated: bool) -> Option<Arc<gpui::Image>> {
    static SIZED: OnceLock<Mutex<HashMap<(String, u32, bool), Option<Arc<gpui::Image>>>>> = OnceLock::new();
    let key = (name.to_string(), edge, pixelated);
    if let Some(found) = SIZED.get_or_init(Default::default).lock().ok()?.get(&key) {
        return found.clone();
    }
    let made = logo_bytes(name).and_then(|bytes| {
        let png = scale_logo(&bytes, edge, pixelated)?;
        Some(Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, png)))
    });
    SIZED.get_or_init(Default::default).lock().ok()?.insert(key, made.clone());
    made
}

/// `bytes` (any format `image` reads) as a PNG whose longer side is at most `edge`.
pub fn scale_logo(bytes: &[u8], edge: u32, pixelated: bool) -> Option<Vec<u8>> {
    let decoded = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = decoded.dimensions();
    let longest = w.max(h).max(1);
    let edge = edge.max(1);
    let scaled = if longest <= edge {
        decoded
    } else {
        let (sw, sh) = ((w * edge / longest).max(1), (h * edge / longest).max(1));
        let filter = if pixelated { FilterType::Nearest } else { FilterType::Lanczos3 };
        image::imageops::resize(&decoded, sw, sh, filter)
    };
    let mut out = Vec::new();
    scaled.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).ok()?;
    Some(out)
}

/// Where a wallpaper image comes from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WallpaperSource {
    /// A theme wallpaper by file name, embedded in the binary.
    Embedded(&'static str),
    /// A file the user picked.
    File(PathBuf),
    /// A diagonal blend through three `0xrrggbb` colours, drawn when there is no image.
    Gradient([u32; 3]),
    /// Another source redrawn with an ordered dither in a few colours.
    Dithered(Box<WallpaperSource>, Dither),
}

/// An ordered (Bayer) dither: the image's brightness drawn with only `palette`'s colours, darkest
/// first, in square dots `pixel` px wide.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Dither {
    pub palette: Vec<u32>,
    pub pixel: u32,
}

impl WallpaperSource {
    /// A theme's background: its embedded wallpaper or wallpaper file, else its gradient.
    pub fn theme(theme: &crate::ui::theme::Theme) -> Self {
        if let Some((name, _)) = EMBEDDED_WALLPAPERS.iter().find(|(n, _)| *n == theme.wallpaper) {
            return Self::Embedded(name);
        }
        let path = Path::new(&theme.wallpaper);
        if !theme.wallpaper.is_empty() && path.is_file() {
            return Self::File(path.to_path_buf());
        }
        let hex = |c: crate::ui::theme::Rgba| (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b);
        Self::Gradient(theme.gradient.map(hex))
    }

    /// Decodes the image and scales it to at most [`MAX_WIDTH`]. `None` if it can't be read.
    fn decode(&self) -> Option<RgbaImage> {
        let decoded = match self {
            Self::Embedded(name) => image::load_from_memory(wallpaper_bytes(name)?),
            Self::File(path) => image::open(path),
            Self::Gradient(stops) => return Some(gradient_image(*stops)),
            Self::Dithered(base, dither) => return Some(dither_image(&base.decode()?, dither)),
        }
        .ok()?
        .into_rgba8();
        let (width, height) = decoded.dimensions();
        if width <= MAX_WIDTH {
            return Some(decoded);
        }
        let scaled_height = (u64::from(height) * u64::from(MAX_WIDTH) / u64::from(width)).max(1) as u32;
        Some(image::imageops::resize(&decoded, MAX_WIDTH, scaled_height, FilterType::Triangle))
    }
}

/// Size of a generated gradient background. Soft blends scale up without visible steps.
const GRADIENT_SIZE: (u32, u32) = (640, 360);

/// A mesh-style background: `stops[0]` as the base, with soft glows of `stops[1]` from the top-left
/// and `stops[2]` from the bottom-right, and a fainter mix of both off-centre. A little noise keeps the
/// blend from banding on 8-bit screens.
fn gradient_image(stops: [u32; 3]) -> RgbaImage {
    let rgb = |c: u32| [(c >> 16) as f32, ((c >> 8) & 0xff) as f32, (c & 0xff) as f32];
    let [base, a, b] = stops.map(rgb);
    let mid = [(a[0] + b[0]) / 2., (a[1] + b[1]) / 2., (a[2] + b[2]) / 2.];
    let (w, h) = GRADIENT_SIZE;
    // (centre x, centre y, radius) as fractions of the width, then the colour and its strength.
    let glows = [((0.08, 0.05, 0.85), a, 0.95), ((0.95, 0.98, 0.9), b, 0.95), ((0.72, 0.3, 0.45), mid, 0.4)];
    RgbaImage::from_fn(w, h, |x, y| {
        let (fx, fy) = (x as f32 / w as f32, y as f32 / w as f32);
        let mut c = base;
        for ((cx, cy, r), colour, strength) in glows {
            let d = ((fx - cx).powi(2) + (fy - cy * h as f32 / w as f32).powi(2)).sqrt() / r;
            // A smooth falloff: full at the centre, gone at the radius.
            let t = (1. - d).clamp(0., 1.);
            let k = t * t * (3. - 2. * t) * strength;
            for i in 0..3 {
                c[i] += (colour[i] - c[i]) * k;
            }
        }
        // ±0.5 levels of hashed noise, below what the eye sees but enough to break bands.
        let n = ((x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663)) % 1000) as f32 / 1000. - 0.5;
        let ch = |v: f32| (v + n).round().clamp(0., 255.) as u8;
        image::Rgba([ch(c[0]), ch(c[1]), ch(c[2]), 255])
    })
}

/// The 8×8 Bayer matrix: thresholds 0–63 spread so neighbouring dots differ as much as possible.
const BAYER8: [[u8; 8]; 8] = [
    [0, 32, 8, 40, 2, 34, 10, 42],
    [48, 16, 56, 24, 50, 18, 58, 26],
    [12, 44, 4, 36, 14, 46, 6, 38],
    [60, 28, 52, 20, 62, 30, 54, 22],
    [3, 35, 11, 43, 1, 33, 9, 41],
    [51, 19, 59, 27, 49, 17, 57, 25],
    [15, 47, 7, 39, 13, 45, 5, 37],
    [63, 31, 55, 23, 61, 29, 53, 21],
];

/// `image` drawn with `dither`'s palette. Brightness is stretched between the image's own darkest and
/// lightest tones so every palette colour is used, then each dot picks between the two nearest palette
/// colours by the Bayer threshold. The result is scaled back up without smoothing, so dots stay square.
fn dither_image(image: &RgbaImage, dither: &Dither) -> RgbaImage {
    let palette: Vec<[u8; 3]> =
        dither.palette.iter().map(|c| [(c >> 16) as u8, (c >> 8) as u8, *c as u8]).collect();
    if palette.len() < 2 {
        return image.clone();
    }
    let pixel = dither.pixel.clamp(1, 16);
    // Dots are measured against a full-width image, so a small source (a generated gradient) gets dots
    // of the same size as a photo, not ones stretched with it.
    let (w, h) = match image.dimensions() {
        (w, h) if w < MAX_WIDTH => (MAX_WIDTH, (u64::from(h) * u64::from(MAX_WIDTH) / u64::from(w)) as u32),
        size => size,
    };
    let (sw, sh) = ((w / pixel).max(1), (h / pixel).max(1));
    let small = image::imageops::resize(image, sw, sh, FilterType::Triangle);
    let luma = |p: &image::Rgba<u8>| (0.2126 * f32::from(p[0]) + 0.7152 * f32::from(p[1]) + 0.0722 * f32::from(p[2])) / 255.;
    // The 2nd and 98th percentiles, so a few extreme dots don't flatten everything else.
    let mut levels: Vec<f32> = small.pixels().map(luma).collect();
    levels.sort_by(f32::total_cmp);
    let (lo, hi) = (levels[levels.len() / 50], levels[levels.len() * 49 / 50]);
    let span = (hi - lo).max(1e-3);
    let steps = (palette.len() - 1) as f32;
    let dotted = RgbaImage::from_fn(sw, sh, |x, y| {
        let v = ((luma(small.get_pixel(x, y)) - lo) / span).clamp(0., 1.) * steps;
        let base = v.floor();
        let threshold = (f32::from(BAYER8[(y % 8) as usize][(x % 8) as usize]) + 0.5) / 64.;
        let index = (base as usize + usize::from(v - base > threshold)).min(palette.len() - 1);
        let [r, g, b] = palette[index];
        image::Rgba([r, g, b, 255])
    });
    image::imageops::resize(&dotted, sw * pixel, sh * pixel, FilterType::Nearest)
}

/// One cached image: the source plus the look, with floats quantized to 0.01 so that float noise
/// doesn't miss the cache. The variant is built from these quantized values, so it's deterministic.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Variant {
    source: WallpaperSource,
    /// The theme's layer blur (`Wallpaper::blur`, Ambient only), in design px × 100.
    layer_blur: u32,
    /// The layer brightness (`Wallpaper::brightness`, which already includes the dim), × 100.
    brightness: u32,
    /// The glass surface's blur in design px × 100. Zero for the plain layer image.
    surface_blur: u32,
}

impl Variant {
    fn new(source: &WallpaperSource, look: Wallpaper, surface_blur: f32) -> Self {
        Self {
            source: source.clone(),
            layer_blur: quantize(look.blur),
            brightness: quantize(look.brightness),
            surface_blur: quantize(surface_blur),
        }
    }
}

fn quantize(value: f32) -> u32 {
    (value.max(0.0) * 100.0).round() as u32
}

fn unquantize(value: u32) -> f32 {
    value as f32 / 100.0
}

/// A variant's state. A failed decode is kept, so a missing file isn't retried every frame.
enum Slot {
    Pending,
    Failed,
    Ready { image: Arc<RenderImage>, used: u64 },
}

#[derive(Default)]
struct State {
    decoded: HashMap<WallpaperSource, Arc<RgbaImage>>,
    images: HashMap<Variant, Slot>,
    clock: u64,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A glass crop's identity: the blurred variant, the canvas it was cut against, the surface in
/// device px, and the layer opacity baked into its alpha.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CropKey {
    variant: Variant,
    canvas: (u32, u32),
    device: DeviceRect,
    opacity: u32,
}

/// A rectangle in device px. Origin top-left, relative to the canvas origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

#[derive(Default)]
struct CropEntries {
    map: HashMap<CropKey, (Arc<RenderImage>, u64)>,
    clock: u64,
}

/// The glass crops, and the images retired from either cache. Retired images wait for the next
/// paint, which frees their GPU textures with `Window::drop_image`.
#[derive(Default)]
struct Crops {
    entries: Mutex<CropEntries>,
    retired: Mutex<Vec<Arc<RenderImage>>>,
}

/// Decodes wallpapers once and blurs them on worker threads, caching the results.
///
/// Calls never block. A variant that isn't ready returns `None` and starts its work; the cache sends
/// a message on the receiver from [`WallpaperCache::new`] when it's ready, so the view can redraw.
pub struct WallpaperCache {
    state: Arc<Mutex<State>>,
    crops: Arc<Crops>,
    ready: smol::channel::Sender<()>,
}

impl WallpaperCache {
    /// A cache and the receiver for ready notices. Await it in a view task and call `cx.notify()`.
    /// Notices coalesce, so a slow receiver never builds up a backlog.
    pub fn new() -> (Self, smol::channel::Receiver<()>) {
        let (ready, notices) = smol::channel::bounded(1);
        (Self { state: Arc::default(), crops: Arc::default(), ready }, notices)
    }

    /// The wallpaper with the layer look baked in: its blur and brightness (dim). `None` while it
    /// decodes, or if the source can't be read.
    pub fn image(&self, source: &WallpaperSource, look: Wallpaper) -> Option<Arc<RenderImage>> {
        self.get(source, look, 0.0)
    }

    /// The layer image blurred by `radius` design px, as seen through a glass surface. `None` while
    /// it's being made.
    pub fn blurred(&self, source: &WallpaperSource, look: Wallpaper, radius: f32) -> Option<Arc<RenderImage>> {
        self.get(source, look, radius)
    }

    fn get(&self, source: &WallpaperSource, look: Wallpaper, surface_blur: f32) -> Option<Arc<RenderImage>> {
        let variant = Variant::new(source, look, surface_blur);
        let mut state = lock(&self.state);
        state.clock += 1;
        let now = state.clock;
        match state.images.get_mut(&variant) {
            Some(Slot::Ready { image, used }) => {
                *used = now;
                Some(Arc::clone(image))
            }
            Some(Slot::Pending | Slot::Failed) => None,
            None => {
                state.images.insert(variant.clone(), Slot::Pending);
                drop(state);
                self.spawn(variant);
                None
            }
        }
    }

    fn spawn(&self, variant: Variant) {
        let state = Arc::clone(&self.state);
        let crops = Arc::clone(&self.crops);
        let ready = self.ready.clone();
        std::thread::spawn(move || {
            let built = build(&state, &variant);
            let mut guard = lock(&state);
            let used = guard.clock;
            let slot = match built {
                Some(image) => Slot::Ready { image, used },
                None => Slot::Failed,
            };
            guard.images.insert(variant, slot);
            let evicted = evict(&mut guard.images);
            drop(guard);
            lock(&crops.retired).extend(evicted);
            let _ = ready.try_send(());
        });
    }
}

/// Decodes the source (or reuses it) and builds the variant. Runs on a worker thread.
fn build(state: &Mutex<State>, variant: &Variant) -> Option<Arc<RenderImage>> {
    let cached = lock(state).decoded.get(&variant.source).cloned();
    let decoded = match cached {
        Some(decoded) => decoded,
        None => {
            let decoded = Arc::new(variant.source.decode()?);
            let mut guard = lock(state);
            if guard.decoded.len() >= DECODED_CAP {
                let evicted = guard.decoded.keys().next().cloned();
                if let Some(key) = evicted {
                    guard.decoded.remove(&key);
                }
            }
            guard.decoded.insert(variant.source.clone(), Arc::clone(&decoded));
            decoded
        }
    };
    let width = decoded.width();
    let layer = process(&decoded, sigma_px(unquantize(variant.layer_blur), width), unquantize(variant.brightness));
    let image = match unquantize(variant.surface_blur) {
        radius if radius > 0.0 => process(&layer, sigma_px(radius, width), 1.0),
        _ => layer,
    };
    Some(render_image(image))
}

/// Keeps at most [`IMAGE_CAP`] ready variants, dropping the least recently used. Returns the dropped
/// images so the caller can free their GPU textures.
fn evict(images: &mut HashMap<Variant, Slot>) -> Vec<Arc<RenderImage>> {
    let mut evicted = Vec::new();
    loop {
        let ready = images.values().filter(|slot| matches!(slot, Slot::Ready { .. })).count();
        if ready <= IMAGE_CAP {
            return evicted;
        }
        let oldest = images
            .iter()
            .filter_map(|(key, slot)| match slot {
                Slot::Ready { used, .. } => Some((*used, key.clone())),
                _ => None,
            })
            .min_by_key(|(used, _)| *used);
        match oldest.and_then(|(_, key)| images.remove(&key)) {
            Some(Slot::Ready { image, .. }) => evicted.push(image),
            _ => return evicted,
        }
    }
}

/// Keeps at most [`CROP_CAP`] crops, retiring the least recently used.
fn evict_crops(entries: &mut CropEntries, retired: &Mutex<Vec<Arc<RenderImage>>>) {
    while entries.map.len() > CROP_CAP {
        let oldest = entries.map.iter().min_by_key(|(_, (_, used))| *used).map(|(key, _)| key.clone());
        let Some(key) = oldest else { return };
        if let Some((image, _)) = entries.map.remove(&key) {
            lock(retired).push(image);
        }
    }
}

/// Gaussian sigma in image px for a blur of `radius` design px on an image `width` px wide.
fn sigma_px(radius: f32, width: u32) -> f32 {
    radius * width as f32 / DESIGN_WIDTH
}

/// Blurs the RGB channels by `sigma` px, then scales them by `brightness`. Alpha is kept.
fn process(img: &RgbaImage, sigma: f32, brightness: f32) -> RgbaImage {
    if sigma < 0.5 && brightness == 1.0 {
        return img.clone();
    }
    let (width, height) = (img.width() as usize, img.height() as usize);
    let mut planes: [Vec<f32>; 3] =
        std::array::from_fn(|channel| img.pixels().map(|p| f32::from(p.0[channel])).collect());
    for plane in &mut planes {
        blur_plane(plane, width, height, sigma);
    }
    let mut out = img.clone();
    for (index, pixel) in out.pixels_mut().enumerate() {
        for (channel, plane) in planes.iter().enumerate() {
            pixel.0[channel] = (plane[index] * brightness).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

/// Blurs one channel in place. Three box passes approximate a Gaussian of standard deviation
/// `sigma`. Edges clamp, so mass is conserved away from the border.
fn blur_plane(plane: &mut [f32], width: usize, height: usize, sigma: f32) {
    if sigma < 0.5 {
        return;
    }
    let mut scratch = vec![0.0; plane.len()];
    let mut column = vec![0.0; height];
    let mut column_out = vec![0.0; height];
    for radius in box_radii(sigma) {
        for (src, dst) in plane.chunks_exact(width).zip(scratch.chunks_exact_mut(width)) {
            box_line(src, dst, radius);
        }
        for x in 0..width {
            for y in 0..height {
                column[y] = scratch[y * width + x];
            }
            box_line(&column, &mut column_out, radius);
            for y in 0..height {
                plane[y * width + x] = column_out[y];
            }
        }
    }
}

/// Radii of three box passes whose combined variance matches a Gaussian of `sigma` (Kutskir's
/// `boxesForGauss`).
fn box_radii(sigma: f32) -> [usize; 3] {
    const N: f32 = 3.0;
    let ideal = (12.0 * sigma * sigma / N + 1.0).sqrt();
    let mut lower = ideal.floor().max(1.0);
    if lower % 2.0 == 0.0 {
        lower -= 1.0;
    }
    let upper = lower + 2.0;
    let m = ((12.0 * sigma * sigma - N * lower * lower - 4.0 * N * lower - 3.0 * N) / (-4.0 * lower - 4.0))
        .round();
    std::array::from_fn(|pass| {
        let size = if (pass as f32) < m { lower } else { upper };
        ((size - 1.0) / 2.0) as usize
    })
}

/// A mean over `2·radius + 1` samples with clamped ends, computed as a running sum.
fn box_line(src: &[f32], dst: &mut [f32], radius: usize) {
    let last = src.len() as isize - 1;
    let at = |i: isize| src[i.clamp(0, last) as usize];
    let r = radius as isize;
    let scale = 1.0 / (2 * radius + 1) as f32;
    let mut sum: f32 = (-r..=r).map(at).sum();
    for (i, out) in dst.iter_mut().enumerate() {
        let i = i as isize;
        *out = sum * scale;
        sum += at(i + r + 1) - at(i - r);
    }
}

/// GPUI's `RenderImage` stores BGRA, so the red and blue channels swap.
fn render_image(mut img: RgbaImage) -> Arc<RenderImage> {
    for pixel in img.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Arc::new(RenderImage::new(smallvec![Frame::new(img)]))
}

/// A rectangle in design px, origin top-left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// The wallpaper layer: the canvas grown by [`WALLPAPER_BLEED`] on every side.
fn layer_rect(canvas: (f32, f32)) -> Rect {
    Rect {
        x: -WALLPAPER_BLEED,
        y: -WALLPAPER_BLEED,
        w: canvas.0 + 2.0 * WALLPAPER_BLEED,
        h: canvas.1 + 2.0 * WALLPAPER_BLEED,
    }
}

/// CSS `center / cover`: scales an image of `image` size to cover `area` and centres it. The result
/// can overhang `area`, which the caller clips.
pub fn cover_rect(image: (f32, f32), area: Rect) -> Rect {
    let scale = (area.w / image.0).max(area.h / image.1);
    let (w, h) = (image.0 * scale, image.1 * scale);
    Rect {
        x: area.x + (area.w - w) / 2.0,
        y: area.y + (area.h - h) / 2.0,
        w,
        h,
    }
}

/// The part of an `image`-sized wallpaper, in image px, that lies under `surface` (canvas design px)
/// when the layer is cover-fitted to `canvas` with its bleed. Clamped to the image.
pub fn crop_source(image: (f32, f32), canvas: (f32, f32), surface: Rect) -> Rect {
    let cover = cover_rect(image, layer_rect(canvas));
    let scale = cover.w / image.0;
    let axis = |start: f32, len: f32, origin: f32, extent: f32| {
        let from = ((start - origin) / scale).clamp(0.0, extent);
        let to = ((start + len - origin) / scale).clamp(0.0, extent);
        (from, to - from)
    };
    let (x, w) = axis(surface.x, surface.w, cover.x, image.0);
    let (y, h) = axis(surface.y, surface.h, cover.y, image.1);
    Rect { x, y, w, h }
}

/// A surface's rectangle at device resolution. Each side is at least one device px.
fn device_rect(surface: Rect, scale: f32) -> DeviceRect {
    DeviceRect {
        x: (surface.x * scale).round() as i32,
        y: (surface.y * scale).round() as i32,
        w: ((surface.w * scale).round() as u32).max(1),
        h: ((surface.h * scale).round() as u32).max(1),
    }
}

/// Cuts `source` (BGRA, `image` px) to `out` px with bilinear sampling, and scales alpha by
/// `opacity`. The output stays BGRA, as GPUI expects. A straight-alpha texture with alpha × opacity
/// is the same as a sprite drawn at that opacity, which GPUI does not expose to elements.
fn resample(bgra: &[u8], image: (u32, u32), source: Rect, out: (u32, u32), opacity: f32) -> RgbaImage {
    let (iw, ih) = (image.0 as usize, image.1 as usize);
    let (out_w, out_h) = (out.0 as usize, out.1 as usize);
    // For each output column or row: the two source indices and the blend weight of the second.
    let taps = |start: f32, len: f32, count: u32, max: usize| -> Vec<(usize, usize, f32)> {
        (0..count)
            .map(|i| {
                let at = (start + (i as f32 + 0.5) * len / count as f32 - 0.5).clamp(0.0, (max - 1) as f32);
                let lo = at.floor();
                (lo as usize, (lo as usize + 1).min(max - 1), at - lo)
            })
            .collect()
    };
    let columns = taps(source.x, source.w, out.0, iw);
    let rows = taps(source.y, source.h, out.1, ih);
    let mut pixels = vec![0u8; out_w * out_h * 4];
    let mut dst = 0;
    for &(y0, y1, fy) in &rows {
        let r0 = y0 * iw * 4;
        let r1 = y1 * iw * 4;
        let ify = 1.0 - fy;
        for &(x0, x1, fx) in &columns {
            let i00 = r0 + x0 * 4;
            let i10 = r0 + x1 * 4;
            let i01 = r1 + x0 * 4;
            let i11 = r1 + x1 * 4;
            let ifx = 1.0 - fx;
            let w00 = ifx * ify;
            let w10 = fx * ify;
            let w01 = ifx * fy;
            let w11 = fx * fy;
            for c in 0..3 {
                let val = f32::from(bgra[i00 + c]) * w00
                    + f32::from(bgra[i10 + c]) * w10
                    + f32::from(bgra[i01 + c]) * w01
                    + f32::from(bgra[i11 + c]) * w11;
                pixels[dst + c] = val.round().clamp(0.0, 255.0) as u8;
            }
            let a = (f32::from(bgra[i00 + 3]) * w00
                + f32::from(bgra[i10 + 3]) * w10
                + f32::from(bgra[i01 + 3]) * w01
                + f32::from(bgra[i11 + 3]) * w11)
                * opacity;
            pixels[dst + 3] = a.round().clamp(0.0, 255.0) as u8;
            dst += 4;
        }
    }
    RgbaImage::from_raw(out.0, out.1, pixels).expect("buffer holds out.0 × out.1 × 4 bytes")
}

fn image_size(image: &RenderImage) -> (f32, f32) {
    let size = image.size(0);
    (size.width.0 as f32, size.height.0 as f32)
}

fn image_dims(image: &RenderImage) -> (u32, u32) {
    let size = image.size(0);
    (size.width.0 as u32, size.height.0 as u32)
}

/// What a view draws its wallpaper and glass with: the cache, which image, the look from
/// `BackgroundStyle::surfaces(..).wallpaper`, and the canvas size.
pub struct Backdrop<'a> {
    pub cache: &'a WallpaperCache,
    pub source: &'a WallpaperSource,
    pub look: Wallpaper,
    /// The canvas size in design px (1920 × 1080 for the shell).
    pub canvas: Size<Pixels>,
}

impl Backdrop<'_> {
    fn canvas(&self) -> (f32, f32) {
        (f32::from(self.canvas.width), f32::from(self.canvas.height))
    }
}

/// An image drawn at exactly `rect` within its parent.
fn placed(image: Arc<RenderImage>, rect: Rect) -> impl IntoElement {
    img(image)
        .absolute()
        .left(px(rect.x))
        .top(px(rect.y))
        .w(px(rect.w))
        .h(px(rect.h))
        .object_fit(ObjectFit::Fill)
}

/// The full-canvas wallpaper: cover-fitted with its bleed and drawn at the look's opacity, which
/// follows a background-style change over 0.4 s (layout.md §6 #1). Empty until the image is ready.
pub fn wallpaper_layer(backdrop: &Backdrop) -> impl IntoElement {
    let image = backdrop.cache.image(backdrop.source, backdrop.look);
    let canvas = backdrop.canvas();
    anim::animated(("wallpaper-layer", 0usize), anim::WALLPAPER_OPACITY, backdrop.look.opacity, move |opacity| {
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .overflow_hidden()
            .opacity(opacity)
            .when_some(image.clone(), |layer, image| {
                let rect = cover_rect(image_size(&image), layer_rect(canvas));
                layer.child(placed(image, rect))
            })
            .into_any_element()
    })
}

/// One glass surface: the blurred wallpaper cropped to its rounded rect, then the fill and the 1 px
/// border. Without a ready variant, only the fill and border are painted.
struct GlassSurface {
    /// The blurred variant the crop is cut from, once it's ready.
    blurred: Option<Arc<RenderImage>>,
    /// Identifies `blurred` in the crop cache key.
    variant: Variant,
    crops: Arc<Crops>,
    canvas: (f32, f32),
    /// The surface in canvas design px.
    surface: Rect,
    /// The layer opacity, baked into the crop's alpha.
    opacity: f32,
    radius: f32,
    fill: Hsla,
    border: Hsla,
}

impl GlassSurface {
    /// The blurred wallpaper under this surface at device resolution: from the cache, or cut now.
    /// `None` until the variant is ready.
    fn crop(&self, scale: f32) -> Option<Arc<RenderImage>> {
        let blurred = self.blurred.as_ref()?;
        let device = device_rect(self.surface, scale);
        let key = CropKey {
            variant: self.variant.clone(),
            canvas: (quantize(self.canvas.0), quantize(self.canvas.1)),
            device,
            opacity: quantize(self.opacity),
        };
        let mut entries = lock(&self.crops.entries);
        entries.clock += 1;
        let now = entries.clock;
        if let Some((image, used)) = entries.map.get_mut(&key) {
            *used = now;
            return Some(Arc::clone(image));
        }
        let source = crop_source(image_size(blurred), self.canvas, self.surface);
        let bgra = blurred.as_bytes(0)?;
        let pixels = resample(bgra, image_dims(blurred), source, (device.w, device.h), self.opacity);
        let image = Arc::new(RenderImage::new(smallvec![Frame::new(pixels)]));
        entries.map.insert(key, (Arc::clone(&image), now));
        evict_crops(&mut entries, &self.crops.retired);
        Some(image)
    }
}

impl IntoElement for GlassSurface {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for GlassSurface {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let style = Style { size: Size::full(), ..Style::default() };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        _window: &mut Window,
        _cx: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        _prepaint: &mut (),
        window: &mut Window,
        _cx: &mut App,
    ) {
        // Images retired by this paint wait for the next one: this frame may still draw them.
        let retired = std::mem::take(&mut *lock(&self.crops.retired));
        for image in retired {
            let _ = window.drop_image(image);
        }
        let corners = Corners::all(px(self.radius)).clamp_radii_for_quad_size(bounds.size);
        // Surfaces are in design units, so a design px is `zoom × scale factor` device px.
        if let Some(crop) = self.crop(window.scale_factor() * crate::ui::scale::zoom()) {
            let _ = window.paint_image(bounds, corners, crop, 0, false);
        }
        window.paint_quad(fill(bounds, self.fill).corner_radii(corners));
        window.paint_quad(
            fill(bounds, transparent_black())
                .corner_radii(corners)
                .border_widths(px(1.))
                .border_color(self.border),
        );
    }
}

/// A glass surface over the wallpaper, at `bounds` in canvas design px. `blur_px` is the surface's
/// blur (the Canvas tile uses 8, the sidebars the panel blur). The blurred wallpaper is cropped to
/// the surface and clipped to its rounded corners by GPUI, so the corners show the blurred
/// wallpaper too.
pub fn glass_surface(
    backdrop: &Backdrop,
    bounds: Rect,
    radius: f32,
    fill: Hsla,
    border: Hsla,
    blur_px: f32,
) -> impl IntoElement {
    let surface = GlassSurface {
        blurred: backdrop.cache.blurred(backdrop.source, backdrop.look, blur_px),
        variant: Variant::new(backdrop.source, backdrop.look, blur_px),
        crops: Arc::clone(&backdrop.cache.crops),
        canvas: backdrop.canvas(),
        surface: bounds,
        opacity: backdrop.look.opacity,
        radius,
        fill,
        border,
    };
    div()
        .absolute()
        .left(px(bounds.x))
        .top(px(bounds.y))
        .w(px(bounds.w))
        .h(px(bounds.h))
        .child(surface)
}

#[cfg(test)]
mod tint_tests {
    use super::tint_logo;

    #[test]
    fn the_glow_takes_the_tint_and_the_blade_stays_white() {
        let mut img = image::RgbaImage::new(2, 1);
        img.put_pixel(0, 0, image::Rgba([230, 30, 30, 255]));
        img.put_pixel(1, 0, image::Rgba([240, 240, 240, 255]));
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let out = image::load_from_memory(&tint_logo(&png, 0x3080ff, None).unwrap()).unwrap().to_rgba8();
        assert_eq!(out.get_pixel(0, 0).0, [30 + 38, 30 + 100, 230, 255], "red glow becomes blue glow");
        assert_eq!(out.get_pixel(1, 0).0, [240, 240, 240, 255]);
        let dark = image::load_from_memory(&tint_logo(&png, 0x3080ff, Some(0x202020)).unwrap()).unwrap().to_rgba8();
        assert_eq!(dark.get_pixel(1, 0).0, [30, 30, 30, 255], "the blade takes the given colour");
        assert_eq!(dark.get_pixel(0, 0).0[2], 4 + 200, "the glow is still the tint");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    fn look(opacity: f32, blur: f32, brightness: f32) -> Wallpaper {
        Wallpaper { opacity, blur, brightness }
    }

    #[test]
    fn dithering_uses_only_the_palette_in_square_dots() {
        let ramp = RgbaImage::from_fn(256, 64, |x, _| image::Rgba([x as u8, x as u8, x as u8, 255]));
        let dither = Dither { palette: vec![0x102030, 0xa0b0c0, 0xf0e0d0], pixel: 4 };
        let out = dither_image(&ramp, &dither);
        let allowed = [[0x10, 0x20, 0x30], [0xa0, 0xb0, 0xc0], [0xf0, 0xe0, 0xd0]];
        assert!(out.pixels().all(|p| allowed.contains(&[p[0], p[1], p[2]])));
        // Every colour shows on a full ramp, and each dot is 4×4.
        for colour in allowed {
            assert!(out.pixels().any(|p| [p[0], p[1], p[2]] == colour));
        }
        assert_eq!(out.get_pixel(0, 0), out.get_pixel(3, 3));
        let source = WallpaperSource::Dithered(Box::new(WallpaperSource::Gradient([0, 0x808080, 0xffffff])), dither);
        assert!(source.decode().is_some());
    }

    #[test]
    fn the_logo_and_gradients_need_no_wallpaper_files() {
        assert!(logo_bytes("archonui-mark.png").is_some());
        assert!(logo_bytes("logo-bm.png").is_none(), "personal logos are files, not embedded");
        assert!(logo_bytes("archonui-mark.png#8fb8e8").is_some(), "the mark in a theme's accent");
        assert!(logo_bytes("archonui-mark.png#nothex").is_none());
        assert!(logo_bytes("archonui-mark.png#0969da#1f2328").is_some(), "with a blade colour");
        assert!(logo_bytes("archonui-mark.png#0969da#nothex").is_none());
        // Logos are scaled to the size they are drawn at, keeping their shape; small ones stay as they are.
        let scaled = image::load_from_memory(&scale_logo(&logo_bytes("archonui-mark.png").unwrap(), 56, false).unwrap()).unwrap();
        assert_eq!((scaled.width(), scaled.height()), (56, 56));
        let mut wide = Vec::new();
        RgbaImage::new(40, 20).write_to(&mut std::io::Cursor::new(&mut wide), image::ImageFormat::Png).unwrap();
        let narrow = image::load_from_memory(&scale_logo(&wide, 10, true).unwrap()).unwrap();
        assert_eq!((narrow.width(), narrow.height()), (10, 5));
        let kept = image::load_from_memory(&scale_logo(&wide, 400, false).unwrap()).unwrap();
        assert_eq!((kept.width(), kept.height()), (40, 20));
        assert!(logo_image("archonui-mark.png", 64, false).is_some());
        assert!(logo_image("no-such-logo.png", 64, false).is_none());
        assert!(wallpaper_bytes("archonui-glow.jpg").is_some(), "the ArchonUI theme's glow ships inside");
        assert!(wallpaper_bytes("bloodmoon2.jpg").is_none(), "personal wallpapers are files");
        // The glows' colours lead in their corners and the base shows between them.
        let blend = gradient_image([0x000000, 0xff0000, 0x0000ff]);
        let (w, h) = GRADIENT_SIZE;
        let [r, _, b, _] = blend.get_pixel(0, 0).0;
        assert!(r > 200 && b < 40, "top-left is the first glow");
        let [r, _, b, _] = blend.get_pixel(w - 1, h - 1).0;
        assert!(b > 200 && r < 40, "bottom-right is the second glow");
        let [r, g, b, _] = blend.get_pixel(w / 4, h - 1).0;
        assert!(r < 90 && g < 10 && b < 90, "the base shows away from the glows: {r} {g} {b}");
    }

    #[test]
    fn layer_is_the_canvas_plus_bleed() {
        assert_eq!(
            layer_rect((1920.0, 1080.0)),
            Rect { x: -60.0, y: -60.0, w: 2040.0, h: 1200.0 }
        );
    }

    #[test]
    fn cover_fits_and_centres_for_wide_and_tall_canvases() {
        let image = (1920.0, 1080.0);
        for canvas in [(1920.0, 1080.0), (1000.0, 1400.0), (3000.0, 800.0)] {
            let layer = layer_rect(canvas);
            let rect = cover_rect(image, layer);
            assert!(rect.x <= layer.x + 1e-3 && rect.x + rect.w >= layer.x + layer.w - 1e-3, "{canvas:?}");
            assert!(rect.y <= layer.y + 1e-3 && rect.y + rect.h >= layer.y + layer.h - 1e-3, "{canvas:?}");
            let fits_width = close(rect.w, layer.w, 1e-3);
            let fits_height = close(rect.h, layer.h, 1e-3);
            assert!(fits_width || fits_height, "neither side meets the layer for {canvas:?}");
            assert!(close(rect.x + rect.w / 2.0, layer.x + layer.w / 2.0, 1e-3), "{canvas:?}");
            assert!(close(rect.y + rect.h / 2.0, layer.y + layer.h / 2.0, 1e-3), "{canvas:?}");
        }
    }

    #[test]
    fn cover_of_a_fhd_image_on_the_fhd_canvas() {
        // 2040 × 1200 layer: height sets the scale, 1200 / 1080 = 10/9.
        let rect = cover_rect((1920.0, 1080.0), layer_rect((1920.0, 1080.0)));
        assert!(close(rect.h, 1200.0, 1e-3));
        assert!(close(rect.w, 1920.0 * 10.0 / 9.0, 1e-2));
        assert!(close(rect.x, -60.0 - (rect.w - 2040.0) / 2.0, 1e-2));
    }

    #[test]
    fn crop_source_follows_cover_offset_and_bleed() {
        // Cover is x −106.67, y −60, scale 10/9. A 300 × 1080 surface at the window's left edge
        // starts 96 image px in and spans 270 × 972 image px.
        let src = crop_source((1920.0, 1080.0), (1920.0, 1080.0), Rect { x: 0.0, y: 0.0, w: 300.0, h: 1080.0 });
        assert!(close(src.x, 96.0, 1e-2) && close(src.y, 54.0, 1e-2), "{src:?}");
        assert!(close(src.w, 270.0, 1e-2) && close(src.h, 972.0, 1e-2), "{src:?}");
    }

    #[test]
    fn crop_source_clamps_to_the_image_at_the_edges() {
        let image = (1920.0, 1080.0);
        let canvas = (1920.0, 1080.0);
        // The whole wallpaper: float noise must not push the crop outside the image.
        let whole = cover_rect(image, layer_rect(canvas));
        let src = crop_source(image, canvas, whole);
        assert!(close(src.x, 0.0, 1e-3) && close(src.y, 0.0, 1e-3), "{src:?}");
        assert!(close(src.w, 1920.0, 1e-2) && close(src.h, 1080.0, 1e-2), "{src:?}");
        // A surface that overhangs the top-left corner starts at image px 0.
        let overhang = crop_source(image, canvas, Rect { x: -200.0, y: -200.0, w: 400.0, h: 400.0 });
        assert!(close(overhang.x, 0.0, 1e-6) && close(overhang.y, 0.0, 1e-6), "{overhang:?}");
        assert!(close(overhang.x + overhang.w, 276.0, 1e-2), "{overhang:?}");
        // A surface that overhangs the bottom-right corner ends at the image edge.
        let right = crop_source(image, canvas, Rect { x: 1800.0, y: 0.0, w: 300.0, h: 1080.0 });
        assert!(close(right.x + right.w, 1920.0, 1e-2), "{right:?}");
    }

    #[test]
    fn device_size_scales_with_the_window_scale_factor() {
        let surface = Rect { x: 10.5, y: 0.0, w: 300.0, h: 1080.0 };
        let one = device_rect(surface, 1.0);
        assert_eq!((one.w, one.h), (300, 1080));
        let two = device_rect(surface, 2.0);
        assert_eq!((two.x, two.w, two.h), (21, 600, 2160));
        assert_ne!(one, two, "scale is part of the crop key");
        assert_eq!(device_rect(surface, 1.5).w, 450);
    }

    #[test]
    fn resample_produces_the_requested_device_size() {
        // The crop is cut at device resolution; the source rect is the same at any scale.
        let bgra = vec![128u8; 8 * 8 * 4];
        let source = crop_source((8.0, 8.0), (8.0, 8.0), Rect { x: -60.0, y: -60.0, w: 2040.0, h: 1200.0 });
        let out = resample(&bgra, (8, 8), source, (4, 6), 1.0);
        assert_eq!(out.dimensions(), (4, 6));
        assert!(out.pixels().all(|p| p.0 == [128, 128, 128, 128]));
    }

    #[test]
    fn resample_at_full_size_is_the_identity() {
        let bgra: Vec<u8> = (0..4 * 4 * 4).map(|i| i as u8 * 3).collect();
        let source = Rect { x: 0.0, y: 0.0, w: 4.0, h: 4.0 };
        let out = resample(&bgra, (4, 4), source, (4, 4), 1.0);
        assert_eq!(out.as_raw(), &bgra);
    }

    #[test]
    fn resample_bakes_opacity_into_alpha_only() {
        let bgra = vec![10u8, 20, 30, 255];
        let out = resample(&bgra, (1, 1), Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }, (1, 1), 0.5);
        assert_eq!(out.as_raw(), &[10, 20, 30, 128]);
    }

    #[test]
    fn crop_cache_retires_the_least_recently_used() {
        let variant = Variant::new(&WallpaperSource::Embedded("bloodmoon2.jpg"), look(1.0, 0.0, 1.0), 24.0);
        let mut entries = CropEntries::default();
        for i in 0..=CROP_CAP as u64 {
            let key = CropKey {
                variant: variant.clone(),
                canvas: (1920, 1080),
                device: DeviceRect { x: i as i32, y: 0, w: 1, h: 1 },
                opacity: 100,
            };
            entries.clock += 1;
            entries.map.insert(key, (render_image(RgbaImage::new(1, 1)), entries.clock));
        }
        let retired = Mutex::new(Vec::new());
        evict_crops(&mut entries, &retired);
        assert_eq!(entries.map.len(), CROP_CAP);
        assert_eq!(retired.lock().unwrap().len(), 1);
        assert!(entries.map.keys().all(|key| key.device.x != 0), "the first, oldest crop is gone");
    }

    #[test]
    fn variant_keys_ignore_opacity_and_float_noise() {
        let source = WallpaperSource::Embedded("bloodmoon2.jpg");
        let a = Variant::new(&source, look(1.0, 0.0, 0.8), 24.0);
        assert_eq!(a, Variant::new(&source, look(0.5, 0.0, 0.8), 24.0));
        assert_eq!(a, Variant::new(&source, look(1.0, 0.0, 0.8 + 1e-6), 24.0));
    }

    #[test]
    fn variant_keys_separate_blur_dim_and_source() {
        let source = WallpaperSource::Embedded("bloodmoon2.jpg");
        let base = Variant::new(&source, look(1.0, 0.0, 1.0), 24.0);
        assert_ne!(base, Variant::new(&source, look(1.0, 0.0, 1.0), 8.0));
        assert_ne!(base, Variant::new(&source, look(1.0, 0.0, 0.8), 24.0));
        assert_ne!(base, Variant::new(&WallpaperSource::File("a.jpg".into()), look(1.0, 0.0, 1.0), 24.0));
        assert_ne!(base, Variant::new(&source, look(1.0, 28.0, 1.0), 24.0));
    }

    #[test]
    fn box_passes_approximate_the_gaussian_variance() {
        // Three box passes of radius r have variance ((2r+1)² − 1)/12 each. Integer widths can only
        // approximate σ²: at σ=2 the error is about 17%, so small blurs get a looser bound.
        for (sigma, tolerance) in [(2.0_f32, 0.2), (8.0, 0.05), (24.0, 0.05)] {
            let variance: f32 = box_radii(sigma)
                .iter()
                .map(|&r| ((2 * r + 1) as f32).powi(2) - 1.0)
                .sum::<f32>()
                / 12.0;
            assert!(close(variance, sigma * sigma, sigma * sigma * tolerance), "σ={sigma} variance={variance}");
        }
    }

    #[test]
    fn white_dot_spreads_and_keeps_its_energy() {
        let (w, h) = (65, 65);
        let mut plane = vec![0.0_f32; w * h];
        plane[32 * w + 32] = 255.0;
        blur_plane(&mut plane, w, h, 3.0);
        let total: f32 = plane.iter().sum();
        assert!((total - 255.0).abs() / 255.0 < 0.01, "energy {total}");
        assert!(plane[32 * w + 33] > 0.0 && plane[32 * w + 33] < plane[32 * w + 32]);
        assert!(close(plane[32 * w + 31], plane[32 * w + 33], 1e-4), "symmetric");
        assert!(plane[32 * w + 32] < 255.0 / 3.0, "spread");
    }

    #[test]
    fn flat_image_stays_flat_at_the_edges() {
        let img = RgbaImage::from_pixel(16, 16, image::Rgba([50, 60, 70, 255]));
        let out = process(&img, 4.0, 1.0);
        assert!(out.pixels().all(|p| p.0 == [50, 60, 70, 255]));
    }

    #[test]
    fn brightness_is_a_multiply_on_rgb_only() {
        let img = RgbaImage::from_pixel(8, 8, image::Rgba([200, 100, 40, 255]));
        let out = process(&img, 0.0, 0.5);
        assert_eq!(out.get_pixel(3, 3).0, [100, 50, 20, 255]);
        let dark = process(&img, 0.0, 0.0);
        assert_eq!(dark.get_pixel(0, 0).0, [0, 0, 0, 255]);
    }

    #[test]
    fn render_image_is_bgra() {
        let img = RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]));
        let render = render_image(img);
        let frame = render.as_bytes(0).unwrap();
        assert_eq!(&frame[..4], &[30, 20, 10, 255]);
    }

    #[test]
    fn cache_reports_loading_then_ready() {
        let path = std::env::temp_dir().join(format!("archonui-glass-test-{}.png", std::process::id()));
        RgbaImage::from_pixel(64, 36, image::Rgba([120, 90, 60, 255])).save(&path).unwrap();
        let source = WallpaperSource::File(path.clone());
        let (cache, notices) = WallpaperCache::new();
        let wall = look(1.0, 0.0, 1.0);
        assert!(cache.blurred(&source, wall, 8.0).is_none(), "first call starts the work");
        smol::block_on(notices.recv()).unwrap();
        let blurred = cache.blurred(&source, wall, 8.0).expect("ready after notice");
        assert_eq!(blurred.size(0).width.0, 64);
        assert!(cache.image(&source, wall).is_none(), "the plain layer is its own variant, not yet built");
        smol::block_on(notices.recv()).unwrap();
        assert!(cache.image(&source, wall).is_some());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn decode_and_blur_timing_gradient() {
        // Run with `cargo test --release glass -- --nocapture` for the numbers in the report.
        let source = WallpaperSource::Gradient([0x0a0a0a, 0x4c1d95, 0xa21caf]);
        let start = std::time::Instant::now();
        let decoded = source.decode().unwrap();
        let decode = start.elapsed();
        let start = std::time::Instant::now();
        let blurred = process(&decoded, sigma_px(24.0, decoded.width()), 1.0);
        let blur = start.elapsed();
        let start = std::time::Instant::now();
        let _ = render_image(blurred);
        let convert = start.elapsed();
        eprintln!(
            "gradient {}x{}: generate {:?}, blur σ=24 {:?}, bgra {:?}",
            decoded.width(),
            decoded.height(),
            decode,
            blur,
            convert
        );
        assert_eq!(decoded.dimensions(), GRADIENT_SIZE);
    }
}
