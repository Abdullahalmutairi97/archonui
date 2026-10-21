//! ArchonUI's colour themes: the 12 presets, the pipeline that resolves them, and the background
//! styles. Spec: `docs/design/themes.md`; `docs/design/themes.json` is its machine-readable twin.

use std::{
    borrow::Cow,
    collections::HashMap,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use serde::{Deserialize, Serialize};

use crate::ui::fonts::{self, FontChoice, FontPair};

/// An 8-bit colour with a float alpha. Opaque colours have `a == 1.0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: f32,
}

impl Rgba {
    pub const fn new(r: u8, g: u8, b: u8, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Opaque colour from `0xrrggbb`.
    pub const fn hex(rgb: u32) -> Self {
        Self::new((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 1.0)
    }

    /// The same colour at alpha `a`, the prototype's `rgba(c, a)`.
    pub const fn alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    /// Parses `#rrggbb` or `rgba(r,g,b,a)`.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if let Some(hex) = s.strip_prefix('#') {
            if hex.len() != 6 || !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }
            return u32::from_str_radix(hex, 16).ok().map(Self::hex);
        }
        let body = s.strip_prefix("rgba(")?.strip_suffix(')')?;
        let parts: Vec<&str> = body.split(',').map(str::trim).collect();
        let [r, g, b, a] = parts.as_slice() else {
            return None;
        };
        let a: f32 = a.parse().ok().filter(|a: &f32| a.is_finite())?;
        Some(Self::new(r.parse().ok()?, g.parse().ok()?, b.parse().ok()?, a.clamp(0.0, 1.0)))
    }

    pub fn to_hsla(self) -> gpui::Hsla {
        gpui::Rgba {
            r: f32::from(self.r) / 255.0,
            g: f32::from(self.g) / 255.0,
            b: f32::from(self.b) / 255.0,
            a: self.a,
        }
        .into()
    }
}

/// Status colours, the same in every theme: working, needs attention, failed.
pub const GREEN: Rgba = Rgba::hex(0x3fb950);
pub const AMBER: Rgba = Rgba::hex(0xd29922);
pub const RED: Rgba = Rgba::hex(0xf85149);

/// The prototype's `mix(a, b, t)`: a per-channel linear blend, rounded. The result is opaque.
pub fn mix(a: Rgba, b: Rgba, t: f64) -> Rgba {
    let lerp = |x: u8, y: u8| (f64::from(x) + (f64::from(y) - f64::from(x)) * t).round() as u8;
    Rgba::new(lerp(a.r, b.r), lerp(a.g, b.g), lerp(a.b, b.b), 1.0)
}

/// A theme's id: `nord`, `claude-code`, or the `id` in a theme file. Settings key by it.
pub type ThemeId = String;

/// `image-rendering` for the logo and hero art. Only Bonfire and Hunter are pixelated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Render {
    Auto,
    Pixelated,
}

/// Blood Moon's hand-tuned overrides, applied after the preset (themes.md §2.3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tune {
    pub tile: Rgba,
    pub panel: Rgba,
    pub pop: Rgba,
    pub fg: Rgba,
    pub muted: Rgba,
    pub faint: Rgba,
    pub soft: Rgba,
    pub acc: Rgba,
}

/// A theme as authored: base colours plus how the window looks. Built-in themes are in [`BUILTIN`];
/// theme files in the config dir's `themes` folder add more (see [`presets`]). `panel`, `pop`,
/// `line`, `line2` and `border` are derived unless `tune` or `surfaces` set them.
#[derive(Debug, Clone)]
pub struct Preset {
    pub id: Cow<'static, str>,
    pub name: Cow<'static, str>,
    pub bg: Rgba,
    pub fg: Rgba,
    pub muted: Rgba,
    pub faint: Rgba,
    pub acc: Rgba,
    pub acc2: Rgba,
    pub deep: Rgba,
    pub alt: Rgba,
    pub on: Rgba,
    pub soft: Rgba,
    /// Hand-tuned overrides applied after the preset (themes.md §2.3).
    pub tune: Option<Tune>,
    /// Tile, sidebar and popover fills, when the theme sets them rather than deriving them.
    pub surfaces: Option<[Rgba; 3]>,
    /// The background image: a file path, or empty for the gradient.
    pub wallpaper: Cow<'static, str>,
    /// The background when there is no image: top-left, centre and bottom-right colours.
    pub gradient: [Rgba; 3],
    /// The logo: an embedded logo's name (see `glass::logo_bytes`) or a file path.
    pub logo: Cow<'static, str>,
    pub render: Render,
    /// Three pairings, the first is the default.
    pub pairings: [FontPair; 3],
}

/// The ArchonUI mark (a plain "A") every built-in theme uses.
pub const DEFAULT_LOGO: &str = "archonui-mark.png";

/// The theme a new install starts with, and the fallback for an id that no longer exists.
pub const DEFAULT_THEME: &str = "archonui";

/// A built-in theme. `colours` is bg, fg, muted, faint, acc, acc2, deep, alt, on; `surfaces` is tile,
/// sidebar and popover; `gradient` is the background's three colours.
#[allow(clippy::too_many_arguments)]
const fn builtin(
    id: &'static str,
    name: &'static str,
    colours: [u32; 9],
    soft: Rgba,
    surfaces: Option<[u32; 3]>,
    gradient: [u32; 3],
    pairings: [FontPair; 3],
) -> Preset {
    let c = colours;
    Preset {
        id: Cow::Borrowed(id),
        name: Cow::Borrowed(name),
        bg: Rgba::hex(c[0]),
        fg: Rgba::hex(c[1]),
        muted: Rgba::hex(c[2]),
        faint: Rgba::hex(c[3]),
        acc: Rgba::hex(c[4]),
        acc2: Rgba::hex(c[5]),
        deep: Rgba::hex(c[6]),
        alt: Rgba::hex(c[7]),
        on: Rgba::hex(c[8]),
        soft,
        tune: None,
        surfaces: match surfaces {
            Some([t, p, o]) => Some([Rgba::hex(t), Rgba::hex(p), Rgba::hex(o)]),
            None => None,
        },
        wallpaper: Cow::Borrowed(""),
        gradient: [Rgba::hex(gradient[0]), Rgba::hex(gradient[1]), Rgba::hex(gradient[2])],
        logo: Cow::Borrowed(DEFAULT_LOGO),
        render: Render::Auto,
        pairings,
    }
}

const fn pair(label: &'static str, display: &'static str, ui: &'static str) -> FontPair {
    FontPair { label, display, ui }
}

/// The themes that ship with ArchonUI: a neutral default, look-alikes of other agent apps, classic
/// editor palettes, and gradient themes whose background is a colour blend.
pub const BUILTIN: &[Preset] = &[
    builtin(
        "archonui",
        "ArchonUI",
        [0x0a0a0a, 0xededed, 0x8a8a8a, 0x3a3a3a, 0xd8d8d8, 0xffffff, 0x4a4a4a, 0xb0b0b0, 0x0a0a0a],
        Rgba::new(255, 255, 255, 0.10),
        Some([0x141414, 0x0f0f0f, 0x1c1c1c]),
        [0x0a0a0a, 0x161616, 0x0a0a0a],
        [pair("Geist", "Geist", "Geist"), pair("Plex", "IBM Plex Sans", "IBM Plex Mono"), pair("Mono", "Geist", "JetBrains Mono")],
    ),
    // Claude's dark mode: warm charcoal, ivory text, the clay accent.
    builtin(
        "claude-code",
        "Claude Code",
        [0x262624, 0xfaf9f5, 0xa6a39a, 0x4a4843, 0xd97757, 0xebb39a, 0x8a3f26, 0xc2c0b6, 0x1f1e1d],
        Rgba::new(217, 119, 87, 0.18),
        Some([0x30302e, 0x1f1e1d, 0x3a3936]),
        [0x262624, 0x2c2b28, 0x22211f],
        [pair("Serif", "Young Serif", "Geist"), pair("Plex", "IBM Plex Sans", "IBM Plex Sans"), pair("Terminal", "Young Serif", "JetBrains Mono")],
    ),
    // ChatGPT's and Codex's dark greys, white controls, blue links.
    builtin(
        "codex",
        "Codex",
        [0x212121, 0xececec, 0xb4b4b4, 0x424242, 0xf3f3f3, 0x339cff, 0x5a5a5a, 0x9b9b9b, 0x0d0d0d],
        Rgba::new(255, 255, 255, 0.10),
        Some([0x2f2f2f, 0x171717, 0x303030]),
        [0x212121, 0x262626, 0x1b1b1b],
        [pair("Geist", "Geist", "Geist"), pair("Plex", "IBM Plex Sans", "IBM Plex Sans"), pair("Mono", "Geist", "Geist Mono")],
    ),
    // Gemini's dark surfaces with its blue to violet to rose spark.
    builtin(
        "gemini",
        "Gemini",
        [0x131314, 0xe3e3e3, 0xc4c7c5, 0x444746, 0xa8c7fa, 0xd7aefb, 0x0b57d0, 0xf2b8b5, 0x062e6f],
        Rgba::new(168, 199, 250, 0.16),
        Some([0x1e1f20, 0x131314, 0x282a2c]),
        [0x131314, 0x1a1d2b, 0x1e1626],
        [pair("DM Sans", "DM Sans", "DM Sans"), pair("Manrope", "Manrope", "Manrope"), pair("Grotesk", "Space Grotesk", "DM Mono")],
    ),
    // Hermes Desktop's default Nous look: a dark blue canvas, cream text, blue and cream accents.
    builtin(
        "hermes",
        "Hermes",
        [0x0c1222, 0xf3ead3, 0xb4ad97, 0x2f3a55, 0x7b9bff, 0xf3ead3, 0x24387a, 0xe09a45, 0x0c1222],
        Rgba::new(107, 140, 255, 0.18),
        Some([0x121a2e, 0x0a0f1d, 0x182239]),
        [0x0c1222, 0x16234a, 0x1c1a2e],
        [pair("Grotesk", "Space Grotesk", "IBM Plex Mono"), pair("Plex", "IBM Plex Sans", "IBM Plex Mono"), pair("Pixel", "Pixelify Sans", "Space Mono")],
    ),
    builtin(
        "nord",
        "Nord",
        [0x2e3440, 0xeceff4, 0xd8dee9, 0x4c566a, 0x88c0d0, 0x81a1c1, 0x5e81ac, 0xa3be8c, 0x2e3440],
        Rgba::new(136, 192, 208, 0.18),
        Some([0x3b4252, 0x2e3440, 0x434c5e]),
        [0x2e3440, 0x343b49, 0x2b303b],
        [pair("Plex", "IBM Plex Sans", "IBM Plex Mono"), pair("Geist", "Geist", "Geist"), pair("Mono", "IBM Plex Sans", "JetBrains Mono")],
    ),
    // Catppuccin Mocha.
    builtin(
        "catppuccin",
        "Catppuccin",
        [0x1e1e2e, 0xcdd6f4, 0xa6adc8, 0x585b70, 0xcba6f7, 0xf5c2e7, 0x7f5ab6, 0x89b4fa, 0x1e1e2e],
        Rgba::new(203, 166, 247, 0.18),
        Some([0x313244, 0x181825, 0x313244]),
        [0x1e1e2e, 0x24243a, 0x181825],
        [pair("Manrope", "Manrope", "Manrope"), pair("DM Sans", "DM Sans", "DM Mono"), pair("Mono", "Manrope", "JetBrains Mono")],
    ),
    builtin(
        "tokyo-night",
        "Tokyo Night",
        [0x1a1b26, 0xc0caf5, 0x9aa5ce, 0x414868, 0x7aa2f7, 0xbb9af7, 0x3d59a1, 0x7dcfff, 0x1a1b26],
        Rgba::new(122, 162, 247, 0.18),
        Some([0x24283b, 0x16161e, 0x292e42]),
        [0x1a1b26, 0x1f2335, 0x16161e],
        [pair("Grotesk", "Space Grotesk", "JetBrains Mono"), pair("Geist", "Geist", "Geist"), pair("Plex", "IBM Plex Sans", "IBM Plex Mono")],
    ),
    // Gruvbox dark.
    builtin(
        "gruvbox",
        "Gruvbox",
        [0x282828, 0xebdbb2, 0xa89984, 0x504945, 0xfe8019, 0xfabd2f, 0xaf3a03, 0x8ec07c, 0x1d2021],
        Rgba::new(254, 128, 25, 0.18),
        Some([0x32302f, 0x1d2021, 0x3c3836]),
        [0x282828, 0x2e2a26, 0x1d2021],
        [pair("Mono", "Space Mono", "JetBrains Mono"), pair("Plex", "IBM Plex Sans", "IBM Plex Mono"), pair("Geist", "Geist", "Geist")],
    ),
    // Gradient themes: the background is a blend of their colours, under translucent surfaces.
    builtin(
        "aurora",
        "Aurora",
        [0x081419, 0xe6f6f4, 0x9cc3c0, 0x2c4a52, 0x5eead4, 0xa78bfa, 0x0f766e, 0x93c5fd, 0x04201c],
        Rgba::new(94, 234, 212, 0.18),
        Some([0x0e1d24, 0x0a171d, 0x13262e]),
        [0x081419, 0x0f5c5a, 0x3b2a78],
        [pair("Grotesk", "Space Grotesk", "Geist"), pair("Manrope", "Manrope", "Manrope"), pair("Mono", "Space Grotesk", "JetBrains Mono")],
    ),
    builtin(
        "sunset",
        "Sunset",
        [0x1a1020, 0xfbe9e0, 0xd2a9a2, 0x4c3040, 0xff9a6b, 0xffc978, 0x9a3550, 0xf58fb8, 0x2a0d12],
        Rgba::new(255, 154, 107, 0.20),
        Some([0x221628, 0x170d1c, 0x2a1b31]),
        [0x1a1020, 0x8a2f55, 0xc8673a],
        [pair("Serif", "Young Serif", "DM Sans"), pair("Manrope", "Manrope", "Manrope"), pair("Bebas", "Bebas Neue", "Manrope")],
    ),
    builtin(
        "ocean",
        "Ocean",
        [0x071521, 0xe2f1fb, 0x93b5cc, 0x23425a, 0x38bdf8, 0x67e8f9, 0x0e4f7a, 0x5eead4, 0x03121d],
        Rgba::new(56, 189, 248, 0.18),
        Some([0x0c1e2c, 0x081a26, 0x112637]),
        [0x071521, 0x0b4a72, 0x0b5e5e],
        [pair("Plex", "IBM Plex Sans", "IBM Plex Sans"), pair("Grotesk", "Space Grotesk", "Space Mono"), pair("Geist", "Geist", "Geist")],
    ),
    builtin(
        "nebula",
        "Nebula",
        [0x120c22, 0xf1e8ff, 0xb7a6d3, 0x43305f, 0xc084fc, 0xf472b6, 0x6b21a8, 0x818cf8, 0x14071f],
        Rgba::new(192, 132, 252, 0.20),
        Some([0x1a1330, 0x0f0a1c, 0x221a3b]),
        [0x120c22, 0x4c1d95, 0x9d1a76],
        [pair("Orbitron", "Orbitron", "Space Mono"), pair("Grotesk", "Space Grotesk", "Geist"), pair("Manrope", "Manrope", "Manrope")],
    ),
];

/// Every theme: the built-in ones, then the theme files in the config dir's `themes` folder, read at
/// startup and again by [`reload_presets`]. A file whose id matches a built-in theme replaces it.
pub fn presets() -> &'static [Preset] {
    let guard = all_presets().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    *guard
}

/// Reads the theme files again, so a theme saved since startup shows up. The previous list is leaked:
/// callers hold `&'static` references into it, and it only happens when a theme is saved.
pub fn reload_presets() {
    let fresh: &'static [Preset] = Box::leak(load_presets().into_boxed_slice());
    *all_presets().lock().unwrap_or_else(std::sync::PoisonError::into_inner) = fresh;
}

fn all_presets() -> &'static std::sync::Mutex<&'static [Preset]> {
    static ALL: OnceLock<std::sync::Mutex<&'static [Preset]>> = OnceLock::new();
    ALL.get_or_init(|| std::sync::Mutex::new(Box::leak(load_presets().into_boxed_slice())))
}

fn load_presets() -> Vec<Preset> {
    let mut all: Vec<Preset> = BUILTIN.to_vec();
    for theme in user_themes(&crate::app::persist::Paths::current().config) {
        match all.iter_mut().find(|p| p.id == theme.id) {
            Some(slot) => *slot = theme,
            None => all.push(theme),
        }
    }
    all
}

/// Whether `id` is one of the themes that come with the app (not a theme file).
pub fn is_builtin(id: &str) -> bool {
    BUILTIN.iter().any(|p| p.id == id)
}

/// The theme file in `config/themes` whose id is `id`, if there is one.
pub fn theme_file(config: &Path, id: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(config.join("themes")).ok()?;
    entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).find(|path| {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .is_some_and(|file| file.get("id").and_then(|v| v.as_str()) == Some(id))
    })
}

/// The `appearance` a saved theme file carries (background, filter, sliders…), if theme `id` has one.
pub fn saved_appearance(config: &Path, id: &str) -> Option<serde_json::Value> {
    let entries = std::fs::read_dir(config.join("themes")).ok()?;
    entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).find_map(|path| {
        let file: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
        (file.get("id")?.as_str()? == id).then(|| file.get("appearance").cloned()).flatten()
    })
}

/// The theme with `id`, or the default when no theme has it (a deleted theme file).
pub fn preset(id: &str) -> &'static Preset {
    let all = presets();
    all.iter().find(|p| p.id == id).or_else(|| all.iter().find(|p| p.id == DEFAULT_THEME)).unwrap_or(&all[0])
}

/// The theme files in `config/themes`, in name order. Files that don't parse are skipped.
pub fn user_themes(config: &Path) -> Vec<Preset> {
    let Ok(entries) = std::fs::read_dir(config.join("themes")) else { return Vec::new() };
    let mut paths: Vec<PathBuf> =
        entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
    paths.sort();
    paths
        .iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(path).ok()?;
            let file: ThemeFile = serde_json::from_str(&text).map_err(|e| eprintln!("theme {}: {e}", path.display())).ok()?;
            file.into_preset(config)
        })
        .collect()
}

/// A theme file: `{"id", "name", "colors": {bg, fg, muted, faint, acc, acc2, deep, alt, on, soft},
/// "tune"?, "gradient"?, "wallpaper"?, "logo"?, "pixelated"?, "fonts"?: [{label, display, ui}]}`.
/// Colours are `#rrggbb` or `rgba(r,g,b,a)`. A relative wallpaper is in `backgrounds`, a relative
/// logo in `logos`, both in the config dir.
#[derive(Deserialize)]
struct ThemeFile {
    id: String,
    name: Option<String>,
    colors: HashMap<String, String>,
    #[serde(default)]
    tune: HashMap<String, String>,
    #[serde(default)]
    gradient: Vec<String>,
    #[serde(default)]
    wallpaper: Option<String>,
    #[serde(default)]
    logo: Option<String>,
    #[serde(default)]
    pixelated: bool,
    #[serde(default)]
    fonts: Vec<ThemeFonts>,
}

#[derive(Deserialize)]
struct ThemeFonts {
    label: String,
    display: String,
    ui: String,
}

impl ThemeFile {
    fn into_preset(self, config: &Path) -> Option<Preset> {
        let colour = |map: &HashMap<String, String>, key: &str| map.get(key).and_then(|c| Rgba::parse(c));
        let base = |key: &str| colour(&self.colors, key);
        let (bg, fg, acc) = (base("bg")?, base("fg")?, base("acc")?);
        let muted = base("muted").unwrap_or_else(|| mix(bg, fg, 0.55));
        let tune = (!self.tune.is_empty()).then(|| {
            let t = |key: &str, fallback: Rgba| colour(&self.tune, key).unwrap_or(fallback);
            Tune {
                tile: t("tile", mix(bg, fg, 0.05)),
                panel: t("panel", mix(bg, fg, 0.02)),
                pop: t("pop", mix(bg, fg, 0.08)),
                fg: t("fg", fg),
                muted: t("muted", muted),
                faint: t("faint", mix(bg, fg, 0.2)),
                soft: t("soft", acc.alpha(0.24)),
                acc: t("acc", acc),
            }
        });
        let gradient = match self.gradient.iter().filter_map(|c| Rgba::parse(c)).collect::<Vec<_>>()[..] {
            [a, b, c] => [a, b, c],
            _ => [bg, mix(bg, fg, 0.05), bg],
        };
        let in_dir = |dir: &str, name: &str| {
            let path = Path::new(name);
            if path.is_absolute() { path.to_path_buf() } else { config.join(dir).join(path) }
        };
        let mut pairings = BUILTIN[0].pairings;
        for (slot, f) in pairings.iter_mut().zip(&self.fonts) {
            *slot = FontPair {
                label: fonts::bundled_family(&f.label).unwrap_or_else(|| fonts::bundled_family(&f.display).unwrap_or("Theme")),
                display: fonts::bundled_family(&f.display).unwrap_or(slot.display),
                ui: fonts::bundled_family(&f.ui).unwrap_or(slot.ui),
            };
        }
        Some(Preset {
            name: Cow::Owned(self.name.clone().unwrap_or_else(|| self.id.clone())),
            id: Cow::Owned(self.id),
            bg,
            fg,
            muted,
            faint: base("faint").unwrap_or_else(|| mix(bg, fg, 0.2)),
            acc,
            acc2: base("acc2").unwrap_or(acc),
            deep: base("deep").unwrap_or_else(|| mix(bg, acc, 0.4)),
            alt: base("alt").unwrap_or(muted),
            on: base("on").unwrap_or(bg),
            soft: base("soft").unwrap_or_else(|| acc.alpha(0.2)),
            tune,
            surfaces: None,
            wallpaper: Cow::Owned(self.wallpaper.map(|w| in_dir("backgrounds", &w).display().to_string()).unwrap_or_default()),
            gradient,
            logo: Cow::Owned(self.logo.map_or_else(|| DEFAULT_LOGO.to_string(), |l| in_dir("logos", &l).display().to_string())),
            render: if self.pixelated { Render::Pixelated } else { Render::Auto },
            pairings,
        })
    }
}

/// The colour tokens a user can override per theme, as `(key, label)` (themes.md §5.1). The first
/// twelve are base tokens; the last five are derived ones (`line`, `line2`, `soft`, and the focus
/// ring's ends), applied after the derivation so they win over it.
pub const EDITABLE_TOKENS: [(&str, &str); 17] = [
    ("bg", "Background"),
    ("tile", "Session tile"),
    ("panel", "Sidebar"),
    ("pop", "Popover"),
    ("fg", "Text"),
    ("muted", "Muted text"),
    ("faint", "Faint"),
    ("on", "Text on accent"),
    ("acc", "Accent"),
    ("acc2", "Highlight"),
    ("alt", "Secondary"),
    ("deep", "Deep accent"),
    ("line", "Borders"),
    ("line2", "Strong borders"),
    ("soft", "Selection"),
    ("ring1", "Highlight start"),
    ("ring2", "Highlight end"),
];

/// The alpha each derived token is drawn at; an override sets its colour and keeps this alpha.
pub const LINE_ALPHA: f32 = 0.09;
pub const LINE2_ALPHA: f32 = 0.22;
pub const SOFT_ALPHA: f32 = 0.28;

/// Alpha of a collapsed-rail band when its session is focused, and when it is not.
const BAND_ALPHA_FOCUSED: f32 = 0.42;
const BAND_ALPHA_IDLE: f32 = 0.20;

/// A fully resolved token set, as the UI draws with it.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub id: ThemeId,
    pub bg: Rgba,
    pub tile: Rgba,
    pub panel: Rgba,
    pub pop: Rgba,
    pub fg: Rgba,
    pub muted: Rgba,
    pub faint: Rgba,
    pub on: Rgba,
    pub acc: Rgba,
    pub acc2: Rgba,
    pub alt: Rgba,
    pub deep: Rgba,
    /// Hairline borders and faint fills: `fg` at 9%.
    pub line: Rgba,
    /// Stronger borders, hover fills and dividers: `fg` at 22%.
    pub line2: Rgba,
    /// Accent wash for selected rows and the user bubble.
    pub soft: Rgba,
    /// Focus-ring gradient stops: `linear-gradient(135deg, [0], [1] 50%, [2])`.
    pub border: [Rgba; 3],
    /// Collapsed-rail ramp from `acc` to `acc2`, opaque. [`Theme::band`] applies the alpha.
    pub bands: [Rgba; 6],
    pub display_font: String,
    pub ui_font: String,
    /// The chat and the message box: the Text font the user picked, or a proportional stand-in when
    /// the theme's own interface font is monospace (see `fonts::reading_font`).
    pub chat_font: String,
    pub code_font: String,
    /// The theme's background image file, or empty for `gradient`.
    pub wallpaper: String,
    /// The background without an image: top-left, centre and bottom-right colours.
    pub gradient: [Rgba; 3],
    /// An embedded logo's name or a logo file's path.
    pub logo: String,
    pub render: Render,
}

impl Theme {
    /// Collapsed-rail tint for session `i` (themes.md §3.6). The ramp repeats every six sessions.
    pub fn band(&self, i: usize, focused: bool) -> Rgba {
        let alpha = if focused { BAND_ALPHA_FOCUSED } else { BAND_ALPHA_IDLE };
        self.bands[i % 6].alpha(alpha)
    }
}

/// The colour tokens after TUNE and before user overrides (`t0` in the prototype).
#[derive(Debug, Clone, Copy)]
struct Tokens {
    bg: Rgba,
    tile: Rgba,
    panel: Rgba,
    pop: Rgba,
    fg: Rgba,
    muted: Rgba,
    faint: Rgba,
    on: Rgba,
    acc: Rgba,
    acc2: Rgba,
    alt: Rgba,
    deep: Rgba,
    soft: Rgba,
}

impl Tokens {
    /// Stages 1–2: the preset, then TUNE (themes.md §2.3).
    fn tuned(p: &Preset) -> Self {
        let (tile, panel, pop, fg, muted, faint, soft, acc) = match (p.tune, p.surfaces) {
            (Some(t), _) => (t.tile, t.panel, t.pop, t.fg, t.muted, t.faint, t.soft, t.acc),
            // Authored surfaces come with authored text colours, so those are used as given.
            (None, Some([tile, panel, pop])) => (tile, panel, pop, p.fg, p.muted, p.faint, p.soft, p.acc),
            (None, None) => (
                mix(p.bg, p.fg, 0.05),
                mix(p.bg, p.fg, 0.02),
                mix(p.bg, p.fg, 0.08),
                p.fg,
                mix(p.muted, p.fg, 0.3),
                mix(p.faint, p.fg, 0.12),
                p.soft,
                p.acc,
            ),
        };
        Self {
            bg: p.bg,
            tile,
            panel,
            pop,
            fg,
            muted,
            faint,
            on: p.on,
            acc,
            acc2: p.acc2,
            alt: p.alt,
            deep: p.deep,
            soft,
        }
    }

    /// Sets an editable token; keys outside [`EDITABLE_TOKENS`] are ignored.
    fn set(&mut self, key: &str, colour: Rgba) {
        match key {
            "bg" => self.bg = colour,
            "tile" => self.tile = colour,
            "panel" => self.panel = colour,
            "pop" => self.pop = colour,
            "fg" => self.fg = colour,
            "muted" => self.muted = colour,
            "faint" => self.faint = colour,
            "on" => self.on = colour,
            "acc" => self.acc = colour,
            "acc2" => self.acc2 = colour,
            "alt" => self.alt = colour,
            "deep" => self.deep = colour,
            _ => {}
        }
    }
}

/// The user's colour for a derived token, or `base`.
fn derived(custom: &HashMap<String, String>, key: &str, base: Rgba) -> Rgba {
    custom.get(key).and_then(|hex| Rgba::parse(hex)).unwrap_or(base)
}

/// Resolves a preset with per-theme colour overrides and a font choice into the rendered tokens
/// (themes.md §2.2). `custom` maps keys from [`EDITABLE_TOKENS`] to `#rrggbb`; other keys and
/// values that do not parse are ignored.
pub fn resolve(
    preset: &Preset,
    custom: &HashMap<String, String>,
    font_choice: FontChoice,
) -> Theme {
    // Stages 1–2: preset, then TUNE.
    let mut t0 = Tokens::tuned(preset);

    // Stage 3: user overrides win over TUNE.
    let mut acc_overridden = false;
    for (key, _) in EDITABLE_TOKENS {
        if let Some(colour) = custom.get(key).and_then(|hex| Rgba::parse(hex)) {
            t0.set(key, colour);
            acc_overridden |= key == "acc";
        }
    }

    // Stage 4: derived tokens, recomputed from the overridden values.
    let soft = if acc_overridden { t0.acc.alpha(0.28) } else { t0.soft };

    // Stage 5: fonts. Wallpaper and logo come from the preset.
    let (display_font, ui_font, code_font) = fonts::resolve_fonts(&preset.pairings, &font_choice);
    let chat_font = if font_choice.ui.is_some() { ui_font.clone() } else { fonts::reading_font(&ui_font) };

    Theme {
        id: preset.id.to_string(),
        bg: t0.bg,
        tile: t0.tile,
        panel: t0.panel,
        pop: t0.pop,
        fg: t0.fg,
        muted: t0.muted,
        faint: t0.faint,
        on: t0.on,
        acc: t0.acc,
        acc2: t0.acc2,
        alt: t0.alt,
        deep: t0.deep,
        line: derived(custom, "line", t0.fg).alpha(LINE_ALPHA),
        line2: derived(custom, "line2", t0.fg).alpha(LINE2_ALPHA),
        soft: custom.get("soft").and_then(|hex| Rgba::parse(hex)).map_or(soft, |c| c.alpha(SOFT_ALPHA)),
        border: [derived(custom, "ring1", t0.acc2), t0.acc, derived(custom, "ring2", t0.deep)],
        bands: std::array::from_fn(|i| mix(t0.acc, t0.acc2, i as f64 * 0.07)),
        display_font,
        ui_font,
        chat_font,
        code_font,
        wallpaper: preset.wallpaper.to_string(),
        gradient: preset.gradient,
        logo: preset.logo.to_string(),
        render: preset.render,
    }
}

/// Background style: how the wallpaper, sidebars and session tiles are layered (themes.md §4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackgroundStyle {
    #[default]
    Canvas,
    Glass,
    Solid,
    Gaps,
    Tinted,
    Ambient,
}

/// The layout sliders that the background formulas read (layout.md §5).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Effects {
    /// Panel blur in px, 0–48.
    pub blur: f32,
    /// Wallpaper dim in percent, 0–80.
    pub dim: f32,
    /// Window opacity, 0.2–1: scales the background, wallpaper, sidebars and sessions so the desktop
    /// shows through. Text and popovers stay opaque.
    pub opacity: f32,
}

impl Default for Effects {
    fn default() -> Self {
        Self { blur: 24.0, dim: 0.0, opacity: 1.0 }
    }
}

impl Effects {
    /// The window opacity within its range, as an alpha.
    pub fn clamped_opacity(self) -> f32 {
        self.clamped().opacity
    }

    /// The sliders within their ranges (blur 0–48, dim 0–80); defaults for a non-number.
    fn clamped(self) -> Self {
        let d = Self::default();
        Self {
            blur: if self.blur.is_finite() { self.blur.clamp(0.0, 48.0) } else { d.blur },
            dim: if self.dim.is_finite() { self.dim.clamp(0.0, 80.0) } else { d.dim },
            opacity: if self.opacity.is_finite() { self.opacity.clamp(0.2, 1.0) } else { d.opacity },
        }
    }
}

/// The wallpaper layer: opacity, then CSS `blur(px)` and `brightness()` filters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Wallpaper {
    pub opacity: f32,
    pub blur: f32,
    pub brightness: f32,
}

/// Wallpaper and sidebar surfaces of a background style.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Surfaces {
    pub wallpaper: Wallpaper,
    /// Fill of both sidebars and the tools panel.
    pub side: Rgba,
    /// Backdrop blur in px, with `saturate(1.2)`. `None` means no backdrop filter.
    pub side_blur: Option<f32>,
}

/// A session tile's fill. `Gradient` runs top to bottom through `(colour, offset 0–1)` stops.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TileFill {
    Solid(Rgba),
    Gradient([(Rgba, f32); 3]),
}

/// A session tile's fill and backdrop blur in px (`None` means no backdrop filter).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TileSurface {
    pub fill: TileFill,
    pub blur: Option<f32>,
}

/// The per-tile state that changes a Canvas tile (themes.md §4.1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TileState {
    pub focused: bool,
    /// The session has no messages yet.
    pub empty: bool,
    /// The "tint all sessions" option is on.
    pub tint_all: bool,
}

/// `c` with its alpha scaled by `by`.
fn faded(c: Rgba, by: f32) -> Rgba {
    Rgba { a: c.a * by, ..c }
}

impl BackgroundStyle {
    /// Wallpaper and sidebar surfaces for this style (themes.md §4).
    pub fn surfaces(self, theme: &Theme, effects: Effects) -> Surfaces {
        let panel = theme.panel;
        // Hand-edited config can put the sliders out of range; a negative brightness would
        // paint the wallpaper inverted and a NaN would poison the cache key.
        let effects = effects.clamped();
        let dim = 1.0 - effects.dim / 100.0;
        let (opacity, blur, brightness, side, side_blur) = match self {
            Self::Canvas => (1.0, 0.0, 1.0, panel.alpha(0.72), Some(effects.blur)),
            Self::Glass => (1.0, 0.0, 1.0, panel.alpha(0.6), Some(effects.blur)),
            Self::Solid => (0.0, 0.0, 1.0, panel, None),
            Self::Gaps => (1.0, 0.0, 1.0, panel.alpha(0.96), None),
            Self::Tinted => (0.5, 0.0, 1.0, panel.alpha(0.55), Some(effects.blur)),
            Self::Ambient => (1.0, 28.0, 0.55, panel.alpha(0.5), Some(effects.blur)),
        };
        // A see-through window keeps the style and fades every layer by the window opacity, so the
        // desktop shows through the wallpaper and surfaces alike.
        let fade = effects.opacity;
        Surfaces {
            wallpaper: Wallpaper { opacity: opacity * fade, blur, brightness: brightness * dim },
            side: faded(side, fade),
            side_blur,
        }
    }

    /// The session tile in this style. Only Canvas varies per tile (themes.md §4.1).
    pub fn tile(self, theme: &Theme, effects: Effects, state: TileState) -> TileSurface {
        let effects = effects.clamped();
        // A see-through window fades the tile with the rest of the window, like `surfaces`.
        let fade = effects.opacity;
        let surface = self.opaque_tile(theme, effects, state);
        let fill = match surface.fill {
            TileFill::Solid(c) => TileFill::Solid(faded(c, fade)),
            TileFill::Gradient(stops) => TileFill::Gradient(stops.map(|(c, at)| (faded(c, fade), at))),
        };
        TileSurface { fill, ..surface }
    }

    /// `tile` at full window opacity.
    fn opaque_tile(self, theme: &Theme, effects: Effects, state: TileState) -> TileSurface {
        let tile = theme.tile;
        let solid = |alpha: f32, blur: Option<f32>| TileSurface {
            fill: TileFill::Solid(tile.alpha(alpha)),
            blur,
        };
        match self {
            Self::Canvas if state.focused && state.empty => solid(0.12, Some(2.0)),
            Self::Canvas if state.focused || state.tint_all => solid(0.82, Some(8.0)),
            Self::Canvas => TileSurface {
                fill: TileFill::Gradient([
                    (tile.alpha(0.25), 0.0),
                    (tile.alpha(0.6), 0.45),
                    (tile.alpha(0.96), 0.85),
                ]),
                blur: None,
            },
            Self::Glass => solid(0.55, Some(effects.blur)),
            Self::Solid | Self::Gaps => TileSurface { fill: TileFill::Solid(tile), blur: None },
            Self::Tinted => solid(0.35, Some((effects.blur / 2.0).round())),
            Self::Ambient => solid(0.45, Some(6.0)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hand-tuned theme as a theme file, so the pipeline tests cover tuning and the file loader.
    const BLOOD_MOON: &str = r##"{
  "id": "blood-moon",
  "name": "Blood Moon",
  "colors": {
    "bg": "#0d0a08",
    "fg": "#eadfcc",
    "muted": "#9a8c78",
    "faint": "#43382c",
    "acc": "#b0141e",
    "acc2": "#d9b56a",
    "deep": "#5a0a10",
    "alt": "#c9a77a",
    "on": "#f6ecdc",
    "soft": "rgba(176,20,30,0.24)"
  },
  "tune": {
    "tile": "#16100d",
    "panel": "#0f0b09",
    "pop": "#1b1411",
    "fg": "#f3ead8",
    "muted": "#bfae94",
    "faint": "#6a5a48",
    "soft": "rgba(176,20,30,0.32)",
    "acc": "#c81e2a"
  },
  "wallpaper": "bloodmoon2.jpg",
  "logo": "logo-bm.png",
  "pixelated": false,
  "fonts": [
    {
      "label": "Fraktur",
      "display": "UnifrakturCook",
      "ui": "JetBrains Mono"
    },
    {
      "label": "Pirata",
      "display": "Pirata One",
      "ui": "Space Mono"
    },
    {
      "label": "Grenze",
      "display": "Grenze Gotisch",
      "ui": "IBM Plex Mono"
    }
  ]
}"##;

    fn resolved(id: &str) -> Theme {
        resolve(preset(id), &HashMap::new(), FontChoice::default())
    }

    fn blood_moon() -> Preset {
        let file: ThemeFile = serde_json::from_str(BLOOD_MOON).unwrap();
        file.into_preset(Path::new("/cfg")).unwrap()
    }

    fn blood() -> Theme {
        resolve(&blood_moon(), &HashMap::new(), FontChoice::default())
    }


    /// WCAG contrast ratio between two colours.
    fn contrast(a: Rgba, b: Rgba) -> f32 {
        let lum = |c: Rgba| {
            let ch = |v: u8| {
                let v = v as f32 / 255.;
                if v <= 0.039_28 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * ch(c.r) + 0.7152 * ch(c.g) + 0.0722 * ch(c.b)
        };
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn notes_on_the_deep_accent_read_better_in_the_body_colour() {
        // The error notes in the Browser, Terminal and Ide tabs sit on `deep`.
        for p in presets() {
            assert!(contrast(p.fg, p.deep) > contrast(p.on, p.deep), "{}", p.id);
            assert!(contrast(p.fg, p.deep) >= 3., "{}: fg on deep is {:.2}", p.id, contrast(p.fg, p.deep));
        }
    }
    #[test]
    fn parse_accepts_hex_and_rgba() {
        assert_eq!(Rgba::parse("#c81e2a"), Some(Rgba::hex(0xc81e2a)));
        assert_eq!(Rgba::parse("rgba(176, 20, 30, 0.32)"), Some(Rgba::new(176, 20, 30, 0.32)));
        assert_eq!(Rgba::parse("#c81e2"), None);
        assert_eq!(Rgba::parse("#gggggg"), None);
        assert_eq!(Rgba::parse("rgba(1,2,3)"), None);
    }

    #[test]
    fn parse_rejects_non_finite_alpha_and_clamps_range() {
        assert_eq!(Rgba::parse("rgba(1,2,3,NaN)"), None);
        assert_eq!(Rgba::parse("rgba(1,2,3,7)"), Some(Rgba::new(1, 2, 3, 1.0)));
    }

    #[test]
    fn mix_rounds_per_channel() {
        assert_eq!(mix(Rgba::hex(0x000000), Rgba::hex(0xffffff), 0.5), Rgba::hex(0x808080));
        assert_eq!(mix(Rgba::hex(0x0d0a08), Rgba::hex(0xeadfcc), 0.0), Rgba::hex(0x0d0a08));
    }

    #[test]
    fn to_hsla_keeps_alpha() {
        let hsla = Rgba::new(255, 255, 255, 0.5).to_hsla();
        assert_eq!((hsla.l, hsla.a), (1.0, 0.5));
    }

    #[test]
    fn blood_moon_resolves_to_spec_values() {
        let t = blood();
        assert_eq!(t.bg, Rgba::hex(0x0d0a08));
        assert_eq!(t.tile, Rgba::hex(0x16100d));
        assert_eq!(t.panel, Rgba::hex(0x0f0b09));
        assert_eq!(t.pop, Rgba::hex(0x1b1411));
        assert_eq!(t.fg, Rgba::hex(0xf3ead8));
        assert_eq!(t.muted, Rgba::hex(0xbfae94));
        assert_eq!(t.faint, Rgba::hex(0x6a5a48));
        assert_eq!(t.acc, Rgba::hex(0xc81e2a));
        assert_eq!(t.acc2, Rgba::hex(0xd9b56a));
        assert_eq!(t.deep, Rgba::hex(0x5a0a10));
        assert_eq!(t.alt, Rgba::hex(0xc9a77a));
        assert_eq!(t.on, Rgba::hex(0xf6ecdc));
        assert_eq!(t.soft, Rgba::new(176, 20, 30, 0.32));
        assert_eq!(t.line, Rgba::new(243, 234, 216, 0.09));
        assert_eq!(t.line2, Rgba::new(243, 234, 216, 0.22));
        assert_eq!(t.border, [Rgba::hex(0xd9b56a), Rgba::hex(0xc81e2a), Rgba::hex(0x5a0a10)]);
        assert_eq!(t.display_font, "UnifrakturCook");
        assert_eq!(t.ui_font, "JetBrains Mono");
        assert_eq!(t.code_font, "JetBrains Mono");
        // A theme file's relative names point into the config dir's folders.
        assert_eq!(t.wallpaper, "/cfg/backgrounds/bloodmoon2.jpg");
        assert_eq!(t.logo, "/cfg/logos/logo-bm.png");
    }

    #[test]
    fn blood_moon_bands_match_spec() {
        let t = blood();
        assert_eq!(t.band(0, true), Rgba::new(200, 30, 42, 0.42));
        assert_eq!(t.band(0, false), Rgba::new(200, 30, 42, 0.20));
        let rgb = [
            (200, 30, 42),
            (201, 41, 46),
            (202, 51, 51),
            (204, 62, 55),
            (205, 72, 60),
            (206, 83, 64),
        ];
        for (i, (r, g, b)) in rgb.into_iter().enumerate() {
            assert_eq!(t.band(i, false), Rgba::new(r, g, b, 0.20), "band {i}");
            assert_eq!(t.band(i + 6, false), t.band(i, false), "band {i} repeats every six");
        }
    }

    #[test]
    fn authored_surfaces_are_used_as_given() {
        let t = resolved("nord");
        assert_eq!((t.bg, t.tile, t.panel, t.pop), (Rgba::hex(0x2e3440), Rgba::hex(0x3b4252), Rgba::hex(0x2e3440), Rgba::hex(0x434c5e)));
        assert_eq!((t.muted, t.faint), (Rgba::hex(0xd8dee9), Rgba::hex(0x4c566a)));
        assert!(t.wallpaper.is_empty(), "built-in themes draw their gradient");
        assert_eq!(t.logo, DEFAULT_LOGO);
    }

    #[test]
    fn custom_accent_rederives_soft_and_border() {
        let custom = HashMap::from([("acc".to_string(), "#ff0000".to_string())]);
        let t = resolve(&blood_moon(), &custom, FontChoice::default());
        assert_eq!(t.acc, Rgba::hex(0xff0000));
        assert_eq!(t.soft, Rgba::new(255, 0, 0, 0.28));
        assert_eq!(t.border[1], Rgba::hex(0xff0000));
        assert_eq!(t.bands[0], Rgba::hex(0xff0000));
        assert_eq!(t.fg, Rgba::hex(0xf3ead8));
    }

    #[test]
    fn custom_text_colour_rederives_lines() {
        let custom = HashMap::from([("fg".to_string(), "#000000".to_string())]);
        let t = resolve(preset("nord"), &custom, FontChoice::default());
        assert_eq!(t.line, Rgba::new(0, 0, 0, 0.09));
        assert_eq!(t.line2, Rgba::new(0, 0, 0, 0.22));
    }

    #[test]
    fn unknown_or_invalid_overrides_are_ignored() {
        let custom = HashMap::from([
            ("bands".to_string(), "#ff0000".to_string()),
            ("acc".to_string(), "not-a-colour".to_string()),
        ]);
        let t = resolve(preset("nord"), &custom, FontChoice::default());
        assert_eq!(t, resolved("nord"));
    }

    #[test]
    fn all_presets_resolve_with_every_pairing() {
        for preset in BUILTIN {
            for pairing in 0..3 {
                let choice = FontChoice { pairing, ..FontChoice::default() };
                let t = resolve(preset, &HashMap::new(), choice);
                assert!(!t.display_font.is_empty() && !t.ui_font.is_empty(), "{:?}", preset.id);
            }
        }
    }

    #[test]
    fn builtin_ids_are_unique_and_unknown_ids_fall_back() {
        let mut ids: Vec<&str> = BUILTIN.iter().map(|p| p.id.as_ref()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), BUILTIN.len());
        assert!(BUILTIN.iter().any(|p| p.id == DEFAULT_THEME));
        assert_eq!(preset("no-such-theme").id, DEFAULT_THEME);
        for id in ["claude-code", "codex", "gemini", "hermes", "nord", "catppuccin", "tokyo-night", "gruvbox"] {
            assert_eq!(preset(id).id, id);
        }
    }

    #[test]
    fn theme_files_load_from_the_config_dir() {
        let dir = std::env::temp_dir().join(format!("archonui-themes-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("themes")).unwrap();
        std::fs::write(dir.join("themes/blood-moon.json"), BLOOD_MOON).unwrap();
        std::fs::write(dir.join("themes/broken.json"), "{ not json").unwrap();
        std::fs::write(dir.join("themes/plain.json"), r##"{"id":"plain","colors":{"bg":"#101010","fg":"#eeeeee","acc":"#ff0000"},"fonts":[{"label":"x","display":"cinzel","ui":"Not A Font"}]}"##).unwrap();
        let themes = user_themes(&dir);
        let ids: Vec<&str> = themes.iter().map(|p| p.id.as_ref()).collect();
        assert_eq!(ids, ["blood-moon", "plain"], "a broken file is skipped");
        let plain = &themes[1];
        assert_eq!(plain.name, "plain");
        assert_eq!(plain.logo, DEFAULT_LOGO);
        // Font names match bundled families in any case; unknown ones keep the default.
        assert_eq!((plain.pairings[0].display, plain.pairings[0].ui), ("Cinzel", BUILTIN[0].pairings[0].ui));
        assert_eq!(plain.wallpaper, "");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn background_styles_round_trip_as_stable_names() {
        assert_eq!(serde_json::to_string(&BackgroundStyle::Canvas).unwrap(), "\"canvas\"");
        for style in [
            BackgroundStyle::Canvas,
            BackgroundStyle::Glass,
            BackgroundStyle::Solid,
            BackgroundStyle::Gaps,
            BackgroundStyle::Tinted,
            BackgroundStyle::Ambient,
        ] {
            let json = serde_json::to_string(&style).unwrap();
            assert_eq!(serde_json::from_str::<BackgroundStyle>(&json).unwrap(), style);
        }
    }

    #[test]
    fn canvas_surfaces_match_blood_moon_spec() {
        let t = blood();
        let surfaces = BackgroundStyle::Canvas.surfaces(&t, Effects::default());
        assert_eq!(surfaces.side, Rgba::new(15, 11, 9, 0.72));
        assert_eq!(surfaces.side_blur, Some(24.0));
        assert_eq!(surfaces.wallpaper, Wallpaper { opacity: 1.0, blur: 0.0, brightness: 1.0 });

        let unfocused = BackgroundStyle::Canvas.tile(&t, Effects::default(), TileState::default());
        assert_eq!(
            unfocused,
            TileSurface {
                fill: TileFill::Gradient([
                    (Rgba::new(22, 16, 13, 0.25), 0.0),
                    (Rgba::new(22, 16, 13, 0.6), 0.45),
                    (Rgba::new(22, 16, 13, 0.96), 0.85),
                ]),
                blur: None,
            }
        );

        let focused_empty = TileState { focused: true, empty: true, tint_all: false };
        let focused = BackgroundStyle::Canvas.tile(&t, Effects::default(), focused_empty);
        assert_eq!(
            focused,
            TileSurface { fill: TileFill::Solid(Rgba::new(22, 16, 13, 0.12)), blur: Some(2.0) }
        );
    }

    #[test]
    fn other_styles_match_blood_moon_spec() {
        let t = blood();
        let effects = Effects::default();
        let state = TileState::default();

        let solid = BackgroundStyle::Solid.surfaces(&t, effects);
        assert_eq!(
            (solid.wallpaper.opacity, solid.side, solid.side_blur),
            (0.0, Rgba::hex(0x0f0b09), None)
        );
        let solid_tile = BackgroundStyle::Solid.tile(&t, effects, state);
        assert_eq!(solid_tile.fill, TileFill::Solid(Rgba::hex(0x16100d)));

        let glass = BackgroundStyle::Glass.tile(&t, effects, state);
        let glass_fill = TileFill::Solid(Rgba::new(22, 16, 13, 0.55));
        assert_eq!(glass, TileSurface { fill: glass_fill, blur: Some(24.0) });

        let tinted = BackgroundStyle::Tinted.surfaces(&t, effects);
        assert_eq!(tinted.wallpaper.opacity, 0.5);
        let tinted_tile = BackgroundStyle::Tinted.tile(&t, effects, state);
        assert_eq!(tinted_tile.blur, Some(12.0));

        let ambient = BackgroundStyle::Ambient.surfaces(&t, effects);
        assert_eq!(ambient.wallpaper, Wallpaper { opacity: 1.0, blur: 28.0, brightness: 0.55 });
        assert_eq!(ambient.side, Rgba::new(15, 11, 9, 0.5));
        assert_eq!(BackgroundStyle::Ambient.tile(&t, effects, state).blur, Some(6.0));
    }

    #[test]
    fn wallpaper_dim_multiplies_brightness() {
        let t = blood();
        let dimmed = Effects { dim: 20.0, ..Effects::default() };
        let canvas = BackgroundStyle::Canvas.surfaces(&t, dimmed);
        assert!((canvas.wallpaper.brightness - 0.8).abs() < 1e-6);
        let ambient = BackgroundStyle::Ambient.surfaces(&t, dimmed);
        assert!((ambient.wallpaper.brightness - 0.44).abs() < 1e-6);
    }

    #[test]
    fn derived_colour_overrides_keep_their_alpha() {
        let preset = blood_moon();
        let custom: HashMap<String, String> =
            [("line", "#ff0000"), ("line2", "#00ff00"), ("soft", "#0000ff"), ("ring1", "#123456"), ("ring2", "#abcdef")]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
        let t = resolve(&preset, &custom, FontChoice::default());
        assert_eq!(t.line, Rgba::hex(0xff0000).alpha(LINE_ALPHA));
        assert_eq!(t.line2, Rgba::hex(0x00ff00).alpha(LINE2_ALPHA));
        assert_eq!(t.soft, Rgba::hex(0x0000ff).alpha(SOFT_ALPHA));
        assert_eq!((t.border[0], t.border[2]), (Rgba::hex(0x123456), Rgba::hex(0xabcdef)));
        // Without overrides the derived tokens follow the base ones.
        let plain = resolve(&preset, &HashMap::new(), FontChoice::default());
        assert_eq!(plain.line, plain.fg.alpha(LINE_ALPHA));
    }

    #[test]
    fn a_see_through_window_keeps_the_style_and_fades_it() {
        let t = blood();
        let half = Effects { opacity: 0.5, ..Effects::default() };
        let state = TileState { focused: true, ..TileState::default() };
        for style in [BackgroundStyle::Canvas, BackgroundStyle::Glass, BackgroundStyle::Solid, BackgroundStyle::Gaps, BackgroundStyle::Ambient] {
            let (full, s) = (style.surfaces(&t, Effects::default()), style.surfaces(&t, half));
            assert_eq!(s.wallpaper.opacity, full.wallpaper.opacity * 0.5, "{style:?}");
            assert_eq!((s.side, s.side_blur), (faded(full.side, 0.5), full.side_blur), "{style:?}");
            let (full_tile, tile) = (style.tile(&t, Effects::default(), state), style.tile(&t, half, state));
            assert_eq!(tile.blur, full_tile.blur, "{style:?}");
            if let (TileFill::Solid(a), TileFill::Solid(b)) = (full_tile.fill, tile.fill) {
                assert_eq!(b, faded(a, 0.5), "{style:?}");
            }
        }
        // Gaps still shows the wallpaper and Solid still hides it.
        assert!(BackgroundStyle::Gaps.surfaces(&t, half).wallpaper.opacity > 0.0);
        assert_eq!(BackgroundStyle::Solid.surfaces(&t, half).wallpaper.opacity, 0.0);
        assert_eq!(Effects { opacity: 0.0, ..Effects::default() }.clamped_opacity(), 0.2, "never fully invisible");
        // At full opacity nothing changes.
        assert_eq!(BackgroundStyle::Canvas.surfaces(&t, Effects::default()).wallpaper.opacity, 1.0);
    }

    #[test]
    fn out_of_range_sliders_are_clamped() {
        let t = blood();
        let wild = Effects { blur: -5.0, dim: 250.0, ..Effects::default() };
        let canvas = BackgroundStyle::Canvas.surfaces(&t, wild);
        assert_eq!(canvas.side_blur, Some(0.0));
        assert!((canvas.wallpaper.brightness - 0.2).abs() < 1e-6);
        let nan = Effects { blur: f32::NAN, dim: f32::NAN, opacity: f32::NAN };
        assert_eq!(BackgroundStyle::Canvas.surfaces(&t, nan).side_blur, Some(24.0));
        assert_eq!(BackgroundStyle::Glass.tile(&t, Effects { blur: 900.0, dim: 0.0, ..Effects::default() }, TileState::default()).blur, Some(48.0));
    }
}
