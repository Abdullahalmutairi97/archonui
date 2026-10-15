//! Bundled fonts and the font choices of the theme pipeline. Spec: `docs/design/themes.md` §6.
//! Family names are the ones GPUI matches; the files are listed in `assets/fonts/README.md`.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

/// One bundled font file: family, CSS weight, italic flag and TTF bytes.
pub type Face = (&'static str, u16, bool, &'static [u8]);

/// Every bundled face, embedded at compile time.
pub const FONTS: &[Face] = &[
    ("Bebas Neue", 400, false, include_bytes!("../../assets/fonts/BebasNeue/BebasNeue-400.ttf")),
    ("Bodoni Moda", 400, false, include_bytes!("../../assets/fonts/BodoniModa/BodoniModa-400.ttf")),
    ("Bodoni Moda", 500, false, include_bytes!("../../assets/fonts/BodoniModa/BodoniModa-500.ttf")),
    ("Bodoni Moda", 600, false, include_bytes!("../../assets/fonts/BodoniModa/BodoniModa-600.ttf")),
    ("Bodoni Moda", 700, false, include_bytes!("../../assets/fonts/BodoniModa/BodoniModa-700.ttf")),
    ("Cinzel", 400, false, include_bytes!("../../assets/fonts/Cinzel/Cinzel-400.ttf")),
    ("Cinzel", 500, false, include_bytes!("../../assets/fonts/Cinzel/Cinzel-500.ttf")),
    ("Cinzel", 600, false, include_bytes!("../../assets/fonts/Cinzel/Cinzel-600.ttf")),
    ("Cinzel", 700, false, include_bytes!("../../assets/fonts/Cinzel/Cinzel-700.ttf")),
    (
        "Cormorant Garamond",
        400,
        false,
        include_bytes!("../../assets/fonts/CormorantGaramond/CormorantGaramond-400.ttf"),
    ),
    (
        "Cormorant Garamond",
        500,
        false,
        include_bytes!("../../assets/fonts/CormorantGaramond/CormorantGaramond-500.ttf"),
    ),
    (
        "Cormorant Garamond",
        600,
        false,
        include_bytes!("../../assets/fonts/CormorantGaramond/CormorantGaramond-600.ttf"),
    ),
    (
        "Cormorant Garamond",
        700,
        false,
        include_bytes!("../../assets/fonts/CormorantGaramond/CormorantGaramond-700.ttf"),
    ),
    ("DM Mono", 400, false, include_bytes!("../../assets/fonts/DMMono/DMMono-400.ttf")),
    ("DM Mono", 500, false, include_bytes!("../../assets/fonts/DMMono/DMMono-500.ttf")),
    ("DM Sans", 400, false, include_bytes!("../../assets/fonts/DMSans/DMSans-400.ttf")),
    ("DM Sans", 500, false, include_bytes!("../../assets/fonts/DMSans/DMSans-500.ttf")),
    ("DM Sans", 600, false, include_bytes!("../../assets/fonts/DMSans/DMSans-600.ttf")),
    ("DM Sans", 700, false, include_bytes!("../../assets/fonts/DMSans/DMSans-700.ttf")),
    ("Fira Code", 400, false, include_bytes!("../../assets/fonts/FiraCode/FiraCode-400.ttf")),
    ("Fira Code", 500, false, include_bytes!("../../assets/fonts/FiraCode/FiraCode-500.ttf")),
    ("Fira Code", 600, false, include_bytes!("../../assets/fonts/FiraCode/FiraCode-600.ttf")),
    ("Fira Code", 700, false, include_bytes!("../../assets/fonts/FiraCode/FiraCode-700.ttf")),
    ("Geist", 400, false, include_bytes!("../../assets/fonts/Geist/Geist-400.ttf")),
    ("Geist", 500, false, include_bytes!("../../assets/fonts/Geist/Geist-500.ttf")),
    ("Geist", 600, false, include_bytes!("../../assets/fonts/Geist/Geist-600.ttf")),
    ("Geist", 700, false, include_bytes!("../../assets/fonts/Geist/Geist-700.ttf")),
    ("Geist", 400, true, include_bytes!("../../assets/fonts/Geist/Geist-Italic-400.ttf")),
    ("Geist Mono", 400, false, include_bytes!("../../assets/fonts/GeistMono/GeistMono-400.ttf")),
    ("Geist Mono", 500, false, include_bytes!("../../assets/fonts/GeistMono/GeistMono-500.ttf")),
    ("Geist Mono", 600, false, include_bytes!("../../assets/fonts/GeistMono/GeistMono-600.ttf")),
    ("Geist Mono", 700, false, include_bytes!("../../assets/fonts/GeistMono/GeistMono-700.ttf")),
    ("Gloock", 400, false, include_bytes!("../../assets/fonts/Gloock/Gloock-400.ttf")),
    (
        "Grenze Gotisch",
        400,
        false,
        include_bytes!("../../assets/fonts/GrenzeGotisch/GrenzeGotisch-400.ttf"),
    ),
    (
        "Grenze Gotisch",
        500,
        false,
        include_bytes!("../../assets/fonts/GrenzeGotisch/GrenzeGotisch-500.ttf"),
    ),
    (
        "Grenze Gotisch",
        600,
        false,
        include_bytes!("../../assets/fonts/GrenzeGotisch/GrenzeGotisch-600.ttf"),
    ),
    (
        "Grenze Gotisch",
        700,
        false,
        include_bytes!("../../assets/fonts/GrenzeGotisch/GrenzeGotisch-700.ttf"),
    ),
    (
        "IBM Plex Mono",
        400,
        false,
        include_bytes!("../../assets/fonts/IBMPlexMono/IBMPlexMono-400.ttf"),
    ),
    (
        "IBM Plex Mono",
        500,
        false,
        include_bytes!("../../assets/fonts/IBMPlexMono/IBMPlexMono-500.ttf"),
    ),
    (
        "IBM Plex Mono",
        600,
        false,
        include_bytes!("../../assets/fonts/IBMPlexMono/IBMPlexMono-600.ttf"),
    ),
    (
        "IBM Plex Mono",
        700,
        false,
        include_bytes!("../../assets/fonts/IBMPlexMono/IBMPlexMono-700.ttf"),
    ),
    (
        "IBM Plex Sans",
        400,
        false,
        include_bytes!("../../assets/fonts/IBMPlexSans/IBMPlexSans-400.ttf"),
    ),
    (
        "IBM Plex Sans",
        500,
        false,
        include_bytes!("../../assets/fonts/IBMPlexSans/IBMPlexSans-500.ttf"),
    ),
    (
        "IBM Plex Sans",
        600,
        false,
        include_bytes!("../../assets/fonts/IBMPlexSans/IBMPlexSans-600.ttf"),
    ),
    (
        "IBM Plex Sans",
        700,
        false,
        include_bytes!("../../assets/fonts/IBMPlexSans/IBMPlexSans-700.ttf"),
    ),
    (
        "IBM Plex Sans",
        400,
        true,
        include_bytes!("../../assets/fonts/IBMPlexSans/IBMPlexSans-Italic-400.ttf"),
    ),
    (
        "Instrument Sans",
        400,
        false,
        include_bytes!("../../assets/fonts/InstrumentSans/InstrumentSans-400.ttf"),
    ),
    (
        "Instrument Sans",
        500,
        false,
        include_bytes!("../../assets/fonts/InstrumentSans/InstrumentSans-500.ttf"),
    ),
    (
        "Instrument Sans",
        600,
        false,
        include_bytes!("../../assets/fonts/InstrumentSans/InstrumentSans-600.ttf"),
    ),
    (
        "Instrument Sans",
        700,
        false,
        include_bytes!("../../assets/fonts/InstrumentSans/InstrumentSans-700.ttf"),
    ),
    ("Italiana", 400, false, include_bytes!("../../assets/fonts/Italiana/Italiana-400.ttf")),
    (
        "JetBrains Mono",
        400,
        false,
        include_bytes!("../../assets/fonts/JetBrainsMono/JetBrainsMono-400.ttf"),
    ),
    (
        "JetBrains Mono",
        500,
        false,
        include_bytes!("../../assets/fonts/JetBrainsMono/JetBrainsMono-500.ttf"),
    ),
    (
        "JetBrains Mono",
        600,
        false,
        include_bytes!("../../assets/fonts/JetBrainsMono/JetBrainsMono-600.ttf"),
    ),
    (
        "JetBrains Mono",
        700,
        false,
        include_bytes!("../../assets/fonts/JetBrainsMono/JetBrainsMono-700.ttf"),
    ),
    ("Manrope", 400, false, include_bytes!("../../assets/fonts/Manrope/Manrope-400.ttf")),
    ("Manrope", 500, false, include_bytes!("../../assets/fonts/Manrope/Manrope-500.ttf")),
    ("Manrope", 600, false, include_bytes!("../../assets/fonts/Manrope/Manrope-600.ttf")),
    ("Manrope", 700, false, include_bytes!("../../assets/fonts/Manrope/Manrope-700.ttf")),
    (
        "Metamorphous",
        400,
        false,
        include_bytes!("../../assets/fonts/Metamorphous/Metamorphous-400.ttf"),
    ),
    (
        "Noto Naskh Arabic",
        400,
        false,
        include_bytes!("../../assets/fonts/NotoNaskhArabic/NotoNaskhArabic-400.ttf"),
    ),
    (
        "Noto Naskh Arabic",
        700,
        false,
        include_bytes!("../../assets/fonts/NotoNaskhArabic/NotoNaskhArabic-700.ttf"),
    ),
    (
        "Noto Sans Arabic",
        400,
        false,
        include_bytes!("../../assets/fonts/NotoSansArabic/NotoSansArabic-400.ttf"),
    ),
    (
        "Noto Sans Arabic",
        700,
        false,
        include_bytes!("../../assets/fonts/NotoSansArabic/NotoSansArabic-700.ttf"),
    ),
    ("Orbitron", 400, false, include_bytes!("../../assets/fonts/Orbitron/Orbitron-400.ttf")),
    ("Orbitron", 500, false, include_bytes!("../../assets/fonts/Orbitron/Orbitron-500.ttf")),
    ("Orbitron", 600, false, include_bytes!("../../assets/fonts/Orbitron/Orbitron-600.ttf")),
    ("Orbitron", 700, false, include_bytes!("../../assets/fonts/Orbitron/Orbitron-700.ttf")),
    (
        "Philosopher",
        400,
        false,
        include_bytes!("../../assets/fonts/Philosopher/Philosopher-400.ttf"),
    ),
    (
        "Philosopher",
        700,
        false,
        include_bytes!("../../assets/fonts/Philosopher/Philosopher-700.ttf"),
    ),
    ("Pirata One", 400, false, include_bytes!("../../assets/fonts/PirataOne/PirataOne-400.ttf")),
    (
        "Pixelify Sans",
        400,
        false,
        include_bytes!("../../assets/fonts/PixelifySans/PixelifySans-400.ttf"),
    ),
    (
        "Pixelify Sans",
        500,
        false,
        include_bytes!("../../assets/fonts/PixelifySans/PixelifySans-500.ttf"),
    ),
    (
        "Pixelify Sans",
        600,
        false,
        include_bytes!("../../assets/fonts/PixelifySans/PixelifySans-600.ttf"),
    ),
    (
        "Pixelify Sans",
        700,
        false,
        include_bytes!("../../assets/fonts/PixelifySans/PixelifySans-700.ttf"),
    ),
    (
        "Press Start 2P",
        400,
        false,
        include_bytes!("../../assets/fonts/PressStart2P/PressStart2P-400.ttf"),
    ),
    ("Silkscreen", 400, false, include_bytes!("../../assets/fonts/Silkscreen/Silkscreen-400.ttf")),
    ("Silkscreen", 700, false, include_bytes!("../../assets/fonts/Silkscreen/Silkscreen-700.ttf")),
    (
        "Space Grotesk",
        400,
        false,
        include_bytes!("../../assets/fonts/SpaceGrotesk/SpaceGrotesk-400.ttf"),
    ),
    (
        "Space Grotesk",
        500,
        false,
        include_bytes!("../../assets/fonts/SpaceGrotesk/SpaceGrotesk-500.ttf"),
    ),
    (
        "Space Grotesk",
        600,
        false,
        include_bytes!("../../assets/fonts/SpaceGrotesk/SpaceGrotesk-600.ttf"),
    ),
    (
        "Space Grotesk",
        700,
        false,
        include_bytes!("../../assets/fonts/SpaceGrotesk/SpaceGrotesk-700.ttf"),
    ),
    ("Space Mono", 400, false, include_bytes!("../../assets/fonts/SpaceMono/SpaceMono-400.ttf")),
    ("Space Mono", 700, false, include_bytes!("../../assets/fonts/SpaceMono/SpaceMono-700.ttf")),
    (
        "UnifrakturCook",
        700,
        false,
        include_bytes!("../../assets/fonts/UnifrakturCook/UnifrakturCook-700.ttf"),
    ),
    ("Young Serif", 400, false, include_bytes!("../../assets/fonts/YoungSerif/YoungSerif-400.ttf")),
    (
        "Zen Old Mincho",
        400,
        false,
        include_bytes!("../../assets/fonts/ZenOldMincho/ZenOldMincho-400.ttf"),
    ),
    (
        "Zen Old Mincho",
        500,
        false,
        include_bytes!("../../assets/fonts/ZenOldMincho/ZenOldMincho-500.ttf"),
    ),
    (
        "Zen Old Mincho",
        600,
        false,
        include_bytes!("../../assets/fonts/ZenOldMincho/ZenOldMincho-600.ttf"),
    ),
    (
        "Zen Old Mincho",
        700,
        false,
        include_bytes!("../../assets/fonts/ZenOldMincho/ZenOldMincho-700.ttf"),
    ),
];

/// Arabic UI family. Cosmic-text's Unix fallback list names this family for the Arabic script, so
/// bundling it keeps Arabic glyphs available without a system font.
pub const ARABIC_UI: &str = "Noto Sans Arabic";

/// Arabic display family, for the serif/display slots.
pub const ARABIC_DISPLAY: &str = "Noto Naskh Arabic";

/// The Arabic fallback chain: the sans face first, Naskh after it.
pub fn arabic_fallbacks() -> gpui::FontFallbacks {
    gpui::FontFallbacks::from_fonts(vec![ARABIC_UI.to_string(), ARABIC_DISPLAY.to_string()])
}

/// A [`gpui::Font`] for a theme family with the Arabic fallbacks attached, so Latin text keeps its
/// theme font and Arabic glyphs come from the bundled Arabic faces.
pub fn themed(family: impl Into<gpui::SharedString>) -> gpui::Font {
    gpui::Font { fallbacks: Some(arabic_fallbacks()), ..gpui::font(family.into()) }
}

/// A font pairing: the display and UI families, with the label on its card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontPair {
    pub label: &'static str,
    pub display: &'static str,
    pub ui: &'static str,
}

/// The six minimal pairs (themes.md §6.2). Picking one sets both the display and UI family.
pub const MINIMAL_PAIRS: [FontPair; 6] = [
    FontPair { label: "Geist", display: "Geist", ui: "Geist" },
    FontPair { label: "IBM Plex", display: "IBM Plex Sans", ui: "IBM Plex Sans" },
    FontPair { label: "DM Sans", display: "DM Sans", ui: "DM Sans" },
    FontPair { label: "Instrument", display: "Instrument Sans", ui: "Geist Mono" },
    FontPair { label: "Manrope", display: "Manrope", ui: "JetBrains Mono" },
    FontPair { label: "Grotesk", display: "Space Grotesk", ui: "Space Mono" },
];

/// The display font list as `(card name, family)` (themes.md §6.2).
pub const DISPLAY_FONTS: [(&str, &str); 23] = [
    ("Geist", "Geist"),
    ("Plex Sans", "IBM Plex Sans"),
    ("DM Sans", "DM Sans"),
    ("Instrument", "Instrument Sans"),
    ("Manrope", "Manrope"),
    ("Grotesk", "Space Grotesk"),
    ("Fraktur", "UnifrakturCook"),
    ("Pirata", "Pirata One"),
    ("Grenze", "Grenze Gotisch"),
    ("Cinzel", "Cinzel"),
    ("Cormorant", "Cormorant Garamond"),
    ("Italiana", "Italiana"),
    ("Bodoni", "Bodoni Moda"),
    ("Gloock", "Gloock"),
    ("Mincho", "Zen Old Mincho"),
    ("Metamorphous", "Metamorphous"),
    ("Young Serif", "Young Serif"),
    ("Philosopher", "Philosopher"),
    ("Bebas", "Bebas Neue"),
    ("Orbitron", "Orbitron"),
    ("Pixelify", "Pixelify Sans"),
    ("Silkscreen", "Silkscreen"),
    ("Press Start", "Press Start 2P"),
];

/// The interface font list (themes.md §6.2). Despite the name it mixes monospace and sans faces.
pub const INTERFACE_FONTS: [&str; 12] = [
    "JetBrains Mono",
    "IBM Plex Mono",
    "Space Mono",
    "DM Mono",
    "Fira Code",
    "Geist Mono",
    "Geist",
    "IBM Plex Sans",
    "DM Sans",
    "Instrument Sans",
    "Manrope",
    "Space Grotesk",
];

/// The code family when the UI family is not monospace.
const GEIST_MONO: &str = "Geist Mono";

/// The font state of one theme: the selected pairing plus per-slot overrides (themes.md §6.1).
/// Pairing cards clear both overrides, minimal pairs set both, the display and interface lists
/// set one each.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FontChoice {
    /// Index into the theme's three pairings; wraps modulo three.
    pub pairing: usize,
    pub display: Option<String>,
    pub ui: Option<String>,
}

/// The display, UI and code families for a theme's pairings and a choice (themes.md §2.5). Code is
/// the UI family when that is monospace, otherwise Geist Mono.
pub fn resolve_fonts(pairings: &[FontPair; 3], choice: &FontChoice) -> (String, String, String) {
    let pair = pairings[choice.pairing % pairings.len()];
    let display = choice.display.clone().unwrap_or_else(|| pair.display.to_string());
    let ui = choice.ui.clone().unwrap_or_else(|| pair.ui.to_string());
    let code = if is_monospace(&ui) { ui.clone() } else { GEIST_MONO.to_string() };
    (display, ui, code)
}

/// Sans family for reply prose when the UI family is monospace: paragraphs read better
/// proportional, and code blocks keep the code font.
const READING_FALLBACK: &str = "Geist";

/// The family chat replies are set in: the UI family, unless that is monospace.
pub fn reading_font(ui: &str) -> String {
    if is_monospace(ui) { READING_FALLBACK.to_string() } else { ui.to_string() }
}

fn is_monospace(family: &str) -> bool {
    let family = family.to_ascii_lowercase();
    family.contains("mono") || family.contains("code")
}

/// Registers every bundled face with GPUI's text system. A face that fails is logged and skipped.
pub fn register(cx: &mut gpui::App) {
    let all: Vec<_> = FONTS.iter().map(|(_, _, _, bytes)| Cow::Borrowed(*bytes)).collect();
    if let Err(err) = cx.text_system().add_fonts(all) {
        eprintln!("archonui: could not register fonts in batch: {err}");
        for (family, weight, italic, bytes) in FONTS {
            if let Err(err) = cx.text_system().add_fonts(vec![Cow::Borrowed(*bytes)]) {
                let style = if *italic { " italic" } else { "" };
                eprintln!("archonui: could not register {family} {weight}{style}: {err}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::{PRESETS, ThemeId};

    #[test]
    fn every_face_is_truetype_or_opentype() {
        for (family, weight, _, bytes) in FONTS {
            let sfnt = bytes.starts_with(&[0, 1, 0, 0])
                || bytes.starts_with(b"OTTO")
                || bytes.starts_with(b"true");
            assert!(sfnt, "{family} {weight} is not an sfnt font");
        }
    }

    #[test]
    fn every_family_in_presets_and_lists_is_embedded() {
        let mut families: Vec<&str> = PRESETS
            .iter()
            .flat_map(|p| p.pairings.iter())
            .flat_map(|p| [p.display, p.ui])
            .collect();
        families.extend(MINIMAL_PAIRS.iter().flat_map(|p| [p.display, p.ui]));
        families.extend(DISPLAY_FONTS.iter().map(|(_, family)| *family));
        families.extend(INTERFACE_FONTS);
        families.push(GEIST_MONO);
        for family in families {
            assert!(FONTS.iter().any(|(f, _, _, _)| *f == family), "{family} is not embedded");
        }
    }

    #[test]
    fn replies_read_in_a_proportional_font() {
        assert_eq!(reading_font("JetBrains Mono"), "Geist");
        assert_eq!(reading_font("Fira Code"), "Geist");
        assert_eq!(reading_font("IBM Plex Sans"), "IBM Plex Sans");
    }

    #[test]
    fn resolve_fonts_applies_pairing_then_overrides() {
        let pairings = &ThemeId::BloodMoon.preset().pairings;
        let default = resolve_fonts(pairings, &FontChoice::default());
        let (d, u, c) = &default;
        assert_eq!(
            (d.as_str(), u.as_str(), c.as_str()),
            ("UnifrakturCook", "JetBrains Mono", "JetBrains Mono")
        );

        let pirata = FontChoice { pairing: 1, ..FontChoice::default() };
        assert_eq!(resolve_fonts(pairings, &pirata).1, "Space Mono");

        // A sans UI family gets Geist Mono for code.
        let minimal = FontChoice {
            pairing: 0,
            display: Some("Geist".to_string()),
            ui: Some("Geist".to_string()),
        };
        let (d, u, c) = resolve_fonts(pairings, &minimal);
        assert_eq!((d.as_str(), u.as_str(), c.as_str()), ("Geist", "Geist", "Geist Mono"));
    }

    #[test]
    fn arabic_families_are_embedded_and_fall_back() {
        for family in [ARABIC_UI, ARABIC_DISPLAY] {
            assert!(FONTS.iter().any(|(f, _, _, _)| *f == family), "{family} is not embedded");
            assert!(FONTS.iter().any(|(f, w, _, _)| *f == family && *w == 700));
        }
        let list = arabic_fallbacks().fallback_list().to_vec();
        assert_eq!(list, vec![ARABIC_UI.to_string(), ARABIC_DISPLAY.to_string()]);
        assert_eq!(themed("Geist").fallbacks, Some(arabic_fallbacks()));
    }

    #[test]
    fn font_choice_round_trips_through_json() {
        let choice = FontChoice { pairing: 2, display: Some("Cinzel".to_string()), ui: None };
        let json = serde_json::to_string(&choice).unwrap();
        assert_eq!(serde_json::from_str::<FontChoice>(&json).unwrap(), choice);
    }
}
