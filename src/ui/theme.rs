//! ArchonUI's colour themes: the 12 presets, the pipeline that resolves them, and the background
//! styles. Spec: `docs/design/themes.md`; `docs/design/themes.json` is its machine-readable twin.

use std::collections::HashMap;

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

/// The prototype's `mix(a, b, t)`: a per-channel linear blend, rounded. The result is opaque.
pub fn mix(a: Rgba, b: Rgba, t: f64) -> Rgba {
    let lerp = |x: u8, y: u8| (f64::from(x) + (f64::from(y) - f64::from(x)) * t).round() as u8;
    Rgba::new(lerp(a.r, b.r), lerp(a.g, b.g), lerp(a.b, b.b), 1.0)
}

/// Stable config id of a preset, serialised kebab-case (`blood-moon`). Order is the prototype's
/// `NAMES` order, which `Alt+T` cycles through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeId {
    ArchonUI,
    #[default]
    BloodMoon,
    Moonlight,
    Wreck,
    Citadel,
    Neon,
    Violet,
    Ink,
    Bonfire,
    Hunter,
    Shards,
    Vale,
}

impl ThemeId {
    /// The preset for this id; [`PRESETS`] is indexed by `ThemeId as usize`.
    pub fn preset(self) -> Preset {
        PRESETS[self as usize]
    }
}

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

/// A preset as authored in the prototype's `T()` factory (L882). `panel`, `pop`, `line`, `line2`
/// and `border` are not stored: the pipeline always replaces them (themes.md §2.3, §2.5).
#[derive(Debug, Clone, Copy)]
pub struct Preset {
    pub id: ThemeId,
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
    /// Only Blood Moon is hand-tuned; the other themes derive tile, panel, pop, muted and faint.
    pub tune: Option<Tune>,
    /// Path under `design/assets/`.
    pub wallpaper: &'static str,
    /// Path under `design/assets/`.
    pub logo: &'static str,
    pub render: Render,
    /// Three pairings, the first is the default.
    pub pairings: [FontPair; 3],
}

/// The 12 presets in the prototype's `NAMES` order (L883–896).
pub const PRESETS: [Preset; 12] = [
    Preset {
        id: ThemeId::ArchonUI,
        bg: Rgba::hex(0x050505),
        fg: Rgba::hex(0xededed),
        muted: Rgba::hex(0x8a8a8a),
        faint: Rgba::hex(0x3a3a3a),
        acc: Rgba::hex(0xd8d8d8),
        acc2: Rgba::hex(0xffffff),
        deep: Rgba::hex(0x4a4a4a),
        alt: Rgba::hex(0xb0b0b0),
        on: Rgba::hex(0x050505),
        soft: Rgba::new(255, 255, 255, 0.12),
        tune: None,
        wallpaper: "fhd/archonui4.jpg",
        logo: "sword-logo-diag.png",
        render: Render::Auto,
        pairings: [
            FontPair { label: "Grenze", display: "Grenze Gotisch", ui: "IBM Plex Mono" },
            FontPair { label: "Bodoni", display: "Bodoni Moda", ui: "JetBrains Mono" },
            FontPair { label: "Italiana", display: "Italiana", ui: "DM Mono" },
        ],
    },
    Preset {
        id: ThemeId::BloodMoon,
        bg: Rgba::hex(0x0d0a08),
        fg: Rgba::hex(0xeadfcc),
        muted: Rgba::hex(0x9a8c78),
        faint: Rgba::hex(0x43382c),
        acc: Rgba::hex(0xb0141e),
        acc2: Rgba::hex(0xd9b56a),
        deep: Rgba::hex(0x5a0a10),
        alt: Rgba::hex(0xc9a77a),
        on: Rgba::hex(0xf6ecdc),
        soft: Rgba::new(176, 20, 30, 0.24),
        tune: Some(Tune {
            tile: Rgba::hex(0x16100d),
            panel: Rgba::hex(0x0f0b09),
            pop: Rgba::hex(0x1b1411),
            fg: Rgba::hex(0xf3ead8),
            muted: Rgba::hex(0xbfae94),
            faint: Rgba::hex(0x6a5a48),
            soft: Rgba::new(176, 20, 30, 0.32),
            acc: Rgba::hex(0xc81e2a),
        }),
        wallpaper: "fhd/bloodmoon2.jpg",
        logo: "logo-bm.png",
        render: Render::Auto,
        pairings: [
            FontPair { label: "Fraktur", display: "UnifrakturCook", ui: "JetBrains Mono" },
            FontPair { label: "Pirata", display: "Pirata One", ui: "Space Mono" },
            FontPair { label: "Grenze", display: "Grenze Gotisch", ui: "IBM Plex Mono" },
        ],
    },
    Preset {
        id: ThemeId::Moonlight,
        bg: Rgba::hex(0x050a14),
        fg: Rgba::hex(0xe3ecf8),
        muted: Rgba::hex(0x8392aa),
        faint: Rgba::hex(0x2c3a52),
        acc: Rgba::hex(0x8fb8e8),
        acc2: Rgba::hex(0xdceaff),
        deep: Rgba::hex(0x33557f),
        alt: Rgba::hex(0xb4c6e0),
        on: Rgba::hex(0x06101f),
        soft: Rgba::new(143, 184, 232, 0.20),
        tune: None,
        wallpaper: "fhd/moonlight2.jpg",
        logo: "logo-moon.png",
        render: Render::Auto,
        pairings: [
            FontPair { label: "Cormorant", display: "Cormorant Garamond", ui: "JetBrains Mono" },
            FontPair { label: "Italiana", display: "Italiana", ui: "DM Mono" },
            FontPair { label: "Bodoni", display: "Bodoni Moda", ui: "IBM Plex Mono" },
        ],
    },
    Preset {
        id: ThemeId::Wreck,
        bg: Rgba::hex(0x1e140a),
        fg: Rgba::hex(0xf3e6cc),
        muted: Rgba::hex(0xa8916c),
        faint: Rgba::hex(0x5a4428),
        acc: Rgba::hex(0xc8893a),
        acc2: Rgba::hex(0xf0c27a),
        deep: Rgba::hex(0x6b4420),
        alt: Rgba::hex(0xe3cfa4),
        on: Rgba::hex(0x1e140a),
        soft: Rgba::new(200, 137, 58, 0.22),
        tune: None,
        wallpaper: "fhd/wreck2.jpg",
        logo: "logo-sand.png",
        render: Render::Auto,
        pairings: [
            FontPair { label: "Bebas", display: "Bebas Neue", ui: "Space Mono" },
            FontPair { label: "Orbitron", display: "Orbitron", ui: "JetBrains Mono" },
            FontPair { label: "Gloock", display: "Gloock", ui: "DM Mono" },
        ],
    },
    Preset {
        id: ThemeId::Citadel,
        bg: Rgba::hex(0x0d0f18),
        fg: Rgba::hex(0xf2e6d0),
        muted: Rgba::hex(0x8f8ea4),
        faint: Rgba::hex(0x383a52),
        acc: Rgba::hex(0xe09a3a),
        acc2: Rgba::hex(0xffcc70),
        deep: Rgba::hex(0x7a3d14),
        alt: Rgba::hex(0x9fb0d8),
        on: Rgba::hex(0x140c04),
        soft: Rgba::new(224, 154, 58, 0.22),
        tune: None,
        wallpaper: "fhd/citadel2.jpg",
        logo: "logo-drg-black.png",
        render: Render::Auto,
        pairings: [
            FontPair { label: "Grenze", display: "Grenze Gotisch", ui: "JetBrains Mono" },
            FontPair { label: "Cinzel", display: "Cinzel", ui: "IBM Plex Mono" },
            FontPair { label: "Fraktur", display: "UnifrakturCook", ui: "Space Mono" },
        ],
    },
    Preset {
        id: ThemeId::Neon,
        bg: Rgba::hex(0x0e0718),
        fg: Rgba::hex(0xf1e6ff),
        muted: Rgba::hex(0x9d8bb8),
        faint: Rgba::hex(0x3d2a5a),
        acc: Rgba::hex(0xb04cff),
        acc2: Rgba::hex(0xff6ad5),
        deep: Rgba::hex(0x4b1d7a),
        alt: Rgba::hex(0x93a8ff),
        on: Rgba::hex(0xffffff),
        soft: Rgba::new(176, 76, 255, 0.24),
        tune: None,
        wallpaper: "fhd/neon2.jpg",
        logo: "logo-cat.png",
        render: Render::Auto,
        pairings: [
            FontPair { label: "Orbitron", display: "Orbitron", ui: "JetBrains Mono" },
            FontPair { label: "Mincho", display: "Zen Old Mincho", ui: "DM Mono" },
            FontPair { label: "Bebas", display: "Bebas Neue", ui: "Fira Code" },
        ],
    },
    Preset {
        id: ThemeId::Violet,
        bg: Rgba::hex(0x0c0a20),
        fg: Rgba::hex(0xece8ff),
        muted: Rgba::hex(0x9590c0),
        faint: Rgba::hex(0x36316a),
        acc: Rgba::hex(0x7a6cff),
        acc2: Rgba::hex(0xc9b8ff),
        deep: Rgba::hex(0x33287a),
        alt: Rgba::hex(0xe08ab8),
        on: Rgba::hex(0xffffff),
        soft: Rgba::new(122, 108, 255, 0.24),
        tune: None,
        wallpaper: "fhd/violet2.jpg",
        logo: "logo-portal.png",
        render: Render::Auto,
        pairings: [
            FontPair { label: "Cormorant", display: "Cormorant Garamond", ui: "JetBrains Mono" },
            FontPair { label: "Metamorphous", display: "Metamorphous", ui: "DM Mono" },
            FontPair { label: "Cinzel", display: "Cinzel", ui: "IBM Plex Mono" },
        ],
    },
    Preset {
        id: ThemeId::Ink,
        bg: Rgba::hex(0xf2f0ea),
        fg: Rgba::hex(0x16130f),
        muted: Rgba::hex(0x6b665d),
        faint: Rgba::hex(0xbdb8ad),
        acc: Rgba::hex(0xc8201e),
        acc2: Rgba::hex(0xe0322a),
        deep: Rgba::hex(0x16130f),
        alt: Rgba::hex(0x3d3a35),
        on: Rgba::hex(0xffffff),
        soft: Rgba::new(200, 32, 30, 0.12),
        tune: None,
        wallpaper: "fhd/ink2.jpg",
        logo: "logo-tree.png",
        render: Render::Auto,
        pairings: [
            FontPair { label: "Mincho", display: "Zen Old Mincho", ui: "DM Mono" },
            FontPair { label: "Cormorant", display: "Cormorant Garamond", ui: "JetBrains Mono" },
            FontPair { label: "Bodoni", display: "Bodoni Moda", ui: "IBM Plex Mono" },
        ],
    },
    Preset {
        id: ThemeId::Bonfire,
        bg: Rgba::hex(0x141b20),
        fg: Rgba::hex(0xdbe6ea),
        muted: Rgba::hex(0x8a9ba3),
        faint: Rgba::hex(0x3a4a52),
        acc: Rgba::hex(0xe8762c),
        acc2: Rgba::hex(0xffb24a),
        deep: Rgba::hex(0x8a3a1c),
        alt: Rgba::hex(0x9cc8d2),
        on: Rgba::hex(0x1a0f08),
        soft: Rgba::new(232, 118, 44, 0.22),
        tune: None,
        wallpaper: "fhd/bonfire.jpg",
        logo: "logo-bonfire.png",
        render: Render::Pixelated,
        pairings: [
            FontPair { label: "Pixelify", display: "Pixelify Sans", ui: "JetBrains Mono" },
            FontPair { label: "Silkscreen", display: "Silkscreen", ui: "Space Mono" },
            FontPair { label: "Press Start", display: "Press Start 2P", ui: "IBM Plex Mono" },
        ],
    },
    Preset {
        id: ThemeId::Hunter,
        bg: Rgba::hex(0x0f1622),
        fg: Rgba::hex(0xe4e8f0),
        muted: Rgba::hex(0x8794ad),
        faint: Rgba::hex(0x323d55),
        acc: Rgba::hex(0xc23a3a),
        acc2: Rgba::hex(0xf0e2c0),
        deep: Rgba::hex(0x6e1e24),
        alt: Rgba::hex(0x8fa6cc),
        on: Rgba::hex(0x0f1622),
        soft: Rgba::new(194, 58, 58, 0.24),
        tune: None,
        wallpaper: "fhd/hunter2.jpg",
        logo: "logo-helm.png",
        render: Render::Pixelated,
        pairings: [
            FontPair { label: "Silkscreen", display: "Silkscreen", ui: "JetBrains Mono" },
            FontPair { label: "Pixelify", display: "Pixelify Sans", ui: "Space Mono" },
            FontPair { label: "Press Start", display: "Press Start 2P", ui: "IBM Plex Mono" },
        ],
    },
    Preset {
        id: ThemeId::Shards,
        bg: Rgba::hex(0x0a0a0c),
        fg: Rgba::hex(0xeef0f4),
        muted: Rgba::hex(0x8c92a0),
        faint: Rgba::hex(0x353944),
        acc: Rgba::hex(0x9aa4b8),
        acc2: Rgba::hex(0xffffff),
        deep: Rgba::hex(0x3a3f4c),
        alt: Rgba::hex(0xc2c8d6),
        on: Rgba::hex(0x0a0a0c),
        soft: Rgba::new(154, 164, 184, 0.20),
        tune: None,
        wallpaper: "fhd/shards2.jpg",
        logo: "logo-snow.png",
        render: Render::Auto,
        pairings: [
            FontPair { label: "Orbitron", display: "Orbitron", ui: "Space Mono" },
            FontPair { label: "Bebas", display: "Bebas Neue", ui: "JetBrains Mono" },
            FontPair { label: "Italiana", display: "Italiana", ui: "DM Mono" },
        ],
    },
    Preset {
        id: ThemeId::Vale,
        bg: Rgba::hex(0x0d150c),
        fg: Rgba::hex(0xeef2e0),
        muted: Rgba::hex(0x94a283),
        faint: Rgba::hex(0x34442c),
        acc: Rgba::hex(0xd0882e),
        acc2: Rgba::hex(0xf2c36a),
        deep: Rgba::hex(0x5a3a14),
        alt: Rgba::hex(0x8cc06a),
        on: Rgba::hex(0x140c04),
        soft: Rgba::new(208, 136, 46, 0.22),
        tune: None,
        wallpaper: "fhd/vale2.jpg",
        logo: "logo-galaxy.png",
        render: Render::Auto,
        pairings: [
            FontPair { label: "Metamorphous", display: "Metamorphous", ui: "JetBrains Mono" },
            FontPair { label: "Young Serif", display: "Young Serif", ui: "DM Mono" },
            FontPair { label: "Philosopher", display: "Philosopher", ui: "Fira Code" },
        ],
    },
];

/// The 12 colour tokens a user can override per theme, as `(key, label)` (themes.md §5.1).
pub const EDITABLE_TOKENS: [(&str, &str); 12] = [
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
];

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
    /// Path under `design/assets/`.
    pub wallpaper: &'static str,
    /// Path under `design/assets/`.
    pub logo: &'static str,
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
        let (tile, panel, pop, fg, muted, faint, soft, acc) = match p.tune {
            Some(t) => (t.tile, t.panel, t.pop, t.fg, t.muted, t.faint, t.soft, t.acc),
            None => (
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
        id: preset.id,
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
        line: t0.fg.alpha(0.09),
        line2: t0.fg.alpha(0.22),
        soft,
        border: [t0.acc2, t0.acc, t0.deep],
        bands: std::array::from_fn(|i| mix(t0.acc, t0.acc2, i as f64 * 0.07)),
        display_font,
        ui_font,
        chat_font,
        code_font,
        wallpaper: preset.wallpaper,
        logo: preset.logo,
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
}

impl Default for Effects {
    fn default() -> Self {
        Self { blur: 24.0, dim: 0.0 }
    }
}

impl Effects {
    /// The sliders within their ranges (blur 0–48, dim 0–80); defaults for a non-number.
    fn clamped(self) -> Self {
        let d = Self::default();
        Self {
            blur: if self.blur.is_finite() { self.blur.clamp(0.0, 48.0) } else { d.blur },
            dim: if self.dim.is_finite() { self.dim.clamp(0.0, 80.0) } else { d.dim },
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
        Surfaces {
            wallpaper: Wallpaper { opacity, blur, brightness: brightness * dim },
            side,
            side_blur,
        }
    }

    /// The session tile in this style. Only Canvas varies per tile (themes.md §4.1).
    pub fn tile(self, theme: &Theme, effects: Effects, state: TileState) -> TileSurface {
        let effects = effects.clamped();
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

    const THEMES_JSON: &str = include_str!("../../docs/design/themes.json");

    fn resolved(id: ThemeId) -> Theme {
        resolve(&id.preset(), &HashMap::new(), FontChoice::default())
    }

    /// The family name of a CSS stack such as `'Cinzel', serif`.
    fn family(stack: &str) -> &str {
        stack.split(',').next().unwrap().trim().trim_matches('\'')
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
        let t = resolved(ThemeId::BloodMoon);
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
        assert_eq!(t.wallpaper, "fhd/bloodmoon2.jpg");
        assert_eq!(t.logo, "logo-bm.png");
    }

    #[test]
    fn blood_moon_bands_match_spec() {
        let t = resolved(ThemeId::BloodMoon);
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
    fn moonlight_resolves_to_spec_values() {
        let t = resolved(ThemeId::Moonlight);
        assert_eq!(t.bg, Rgba::hex(0x050a14));
        assert_eq!(t.acc, Rgba::hex(0x8fb8e8));
    }

    #[test]
    fn custom_accent_rederives_soft_and_border() {
        let custom = HashMap::from([("acc".to_string(), "#ff0000".to_string())]);
        let t = resolve(&ThemeId::BloodMoon.preset(), &custom, FontChoice::default());
        assert_eq!(t.acc, Rgba::hex(0xff0000));
        assert_eq!(t.soft, Rgba::new(255, 0, 0, 0.28));
        assert_eq!(t.border[1], Rgba::hex(0xff0000));
        assert_eq!(t.bands[0], Rgba::hex(0xff0000));
        assert_eq!(t.fg, Rgba::hex(0xf3ead8));
    }

    #[test]
    fn custom_text_colour_rederives_lines() {
        let custom = HashMap::from([("fg".to_string(), "#000000".to_string())]);
        let t = resolve(&ThemeId::Ink.preset(), &custom, FontChoice::default());
        assert_eq!(t.line, Rgba::new(0, 0, 0, 0.09));
        assert_eq!(t.line2, Rgba::new(0, 0, 0, 0.22));
    }

    #[test]
    fn unknown_or_invalid_overrides_are_ignored() {
        let custom = HashMap::from([
            ("soft".to_string(), "#ff0000".to_string()),
            ("acc".to_string(), "not-a-colour".to_string()),
        ]);
        let t = resolve(&ThemeId::Moonlight.preset(), &custom, FontChoice::default());
        assert_eq!(t, resolved(ThemeId::Moonlight));
    }

    #[test]
    fn all_presets_resolve_with_every_pairing() {
        for preset in &PRESETS {
            for pairing in 0..3 {
                let choice = FontChoice { pairing, ..FontChoice::default() };
                let t = resolve(preset, &HashMap::new(), choice);
                assert!(!t.display_font.is_empty() && !t.ui_font.is_empty(), "{:?}", preset.id);
            }
        }
    }

    #[test]
    fn presets_match_themes_json() {
        let spec: serde_json::Value = serde_json::from_str(THEMES_JSON).unwrap();
        let themes = spec["themes"].as_array().unwrap();
        for preset in &PRESETS {
            let expected = themes.iter().find(|t| t["order"].as_u64() == Some(preset.id as u64));
            let expected = expected.unwrap();
            let t = resolve(preset, &HashMap::new(), FontChoice::default());
            let tokens = &expected["tokens"];
            let colour = |key: &str| Rgba::parse(tokens[key].as_str().unwrap()).unwrap();
            let actual = [
                ("bg", t.bg),
                ("tile", t.tile),
                ("panel", t.panel),
                ("pop", t.pop),
                ("fg", t.fg),
                ("muted", t.muted),
                ("faint", t.faint),
                ("line", t.line),
                ("line2", t.line2),
                ("acc", t.acc),
                ("acc2", t.acc2),
                ("deep", t.deep),
                ("alt", t.alt),
                ("on", t.on),
                ("soft", t.soft),
            ];
            for (key, value) in actual {
                assert_eq!(value, colour(key), "{:?} {key}", preset.id);
            }
            for (i, band) in expected["bands"].as_array().unwrap().iter().enumerate() {
                let band = Rgba::parse(band.as_str().unwrap()).unwrap();
                assert_eq!(t.bands[i], band, "{:?} band {i}", preset.id);
            }
            assert_eq!(t.display_font, family(tokens["display"].as_str().unwrap()));
            assert_eq!(t.ui_font, family(tokens["ui"].as_str().unwrap()));
            assert_eq!(t.code_font, family(tokens["code"].as_str().unwrap()));
            assert_eq!(expected["wallpaper"].as_str().unwrap(), format!("assets/{}", t.wallpaper));
            assert_eq!(expected["logo"].as_str().unwrap(), format!("assets/{}", t.logo));
        }
    }

    #[test]
    fn preset_ids_index_the_preset_table() {
        for (i, preset) in PRESETS.iter().enumerate() {
            assert_eq!(preset.id as usize, i);
        }
    }

    #[test]
    fn config_ids_round_trip_as_stable_names() {
        assert_eq!(serde_json::to_string(&ThemeId::BloodMoon).unwrap(), "\"blood-moon\"");
        assert_eq!(serde_json::to_string(&BackgroundStyle::Canvas).unwrap(), "\"canvas\"");
        for preset in &PRESETS {
            let json = serde_json::to_string(&preset.id).unwrap();
            assert_eq!(serde_json::from_str::<ThemeId>(&json).unwrap(), preset.id);
        }
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
        let t = resolved(ThemeId::BloodMoon);
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
        let t = resolved(ThemeId::BloodMoon);
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
        let t = resolved(ThemeId::BloodMoon);
        let dimmed = Effects { dim: 20.0, ..Effects::default() };
        let canvas = BackgroundStyle::Canvas.surfaces(&t, dimmed);
        assert!((canvas.wallpaper.brightness - 0.8).abs() < 1e-6);
        let ambient = BackgroundStyle::Ambient.surfaces(&t, dimmed);
        assert!((ambient.wallpaper.brightness - 0.44).abs() < 1e-6);
    }

    #[test]
    fn out_of_range_sliders_are_clamped() {
        let t = resolved(ThemeId::BloodMoon);
        let wild = Effects { blur: -5.0, dim: 250.0 };
        let canvas = BackgroundStyle::Canvas.surfaces(&t, wild);
        assert_eq!(canvas.side_blur, Some(0.0));
        assert!((canvas.wallpaper.brightness - 0.2).abs() < 1e-6);
        let nan = Effects { blur: f32::NAN, dim: f32::NAN };
        assert_eq!(BackgroundStyle::Canvas.surfaces(&t, nan).side_blur, Some(24.0));
        assert_eq!(BackgroundStyle::Glass.tile(&t, Effects { blur: 900.0, dim: 0.0 }, TileState::default()).blur, Some(48.0));
    }
}
