# ArchonUI fonts

Static per-weight TrueType (`.ttf`) fonts downloaded from the Google Fonts CSS API on 2026-10-12
(the Arabic fallbacks below come from the `github.com/google/fonts` repository instead; see
[Arabic fallbacks](#arabic-fallbacks)).
Every file was verified with `fc-scan` to be a **non-variable** font (`variable=False`) with the weight implied by its filename.

Request pattern (legacy User-Agent makes the API serve `.ttf` instead of variable `woff2`):

```
curl -s -A "Mozilla/5.0 (Macintosh; U; Intel Mac OS X 10_5_6; en-us) AppleWebKit/525.27.1 (KHTML, like Gecko) Version/3.2.1 Safari/525.27.1" \
  "https://fonts.googleapis.com/css2?family=Manrope:wght@400;500;600;700&subset=latin"
```
Layout: `assets/fonts/<FamilyNoSpaces>/<FamilyNoSpaces>-<Weight>.ttf` (italic: `<FamilyNoSpaces>-Italic-<Weight>.ttf`), licence text beside the files.

**Total size: 8,180,967 bytes (7.80 MiB), 90 `.ttf` files + 31 licence files.** (Zen Old Mincho was subset after the original download; see Subsetting. The Arabic families below are static instances of upstream variable fonts and count in these totals.)

## Display

| Family | Weights downloaded | Licence | Source URL |
| --- | --- | --- | --- |
| UnifrakturCook | 700 | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=UnifrakturCook:wght@400;500;600;700&subset=latin |
| Pirata One | 400 | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=Pirata%20One:wght@400;500;600;700&subset=latin |
| Grenze Gotisch | 400, 500, 600, 700 | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=Grenze%20Gotisch:wght@400;500;600;700&subset=latin |
| Cinzel | 400, 500, 600, 700 | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=Cinzel:wght@400;500;600;700&subset=latin |
| Cormorant Garamond | 400, 500, 600, 700 | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=Cormorant%20Garamond:wght@400;500;600;700&subset=latin |
| Italiana | 400 | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=Italiana:wght@400;500;600;700&subset=latin |
| Bodoni Moda | 400, 500, 600, 700 | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=Bodoni%20Moda:wght@400;500;600;700&subset=latin |
| Gloock | 400 | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=Gloock:wght@400;500;600;700&subset=latin |
| Zen Old Mincho | 400, 500, 600, 700 | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=Zen%20Old%20Mincho:wght@400;500;600;700&subset=latin |
| Metamorphous | 400 | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=Metamorphous:wght@400;500;600;700&subset=latin |
| Young Serif | 400 | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=Young%20Serif:wght@400;500;600;700&subset=latin |
| Philosopher | 400, 700 | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=Philosopher:wght@400;500;600;700&subset=latin |
| Bebas Neue | 400 | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=Bebas%20Neue:wght@400;500;600;700&subset=latin |
| Orbitron | 400, 500, 600, 700 | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=Orbitron:wght@400;500;600;700&subset=latin |
| Pixelify Sans | 400, 500, 600, 700 | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=Pixelify%20Sans:wght@400;500;600;700&subset=latin |
| Silkscreen | 400, 700 | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=Silkscreen:wght@400;500;600;700&subset=latin |
| Press Start 2P | 400 | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=Press%20Start%202P:wght@400;500;600;700&subset=latin |

## Sans

| Family | Weights downloaded | Licence | Source URL |
| --- | --- | --- | --- |
| Geist | 400, 500, 600, 700, 400 italic | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=Geist:wght@400;500;600;700&subset=latin |
| IBM Plex Sans | 400, 500, 600, 700, 400 italic | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=IBM%20Plex%20Sans:wght@400;500;600;700&subset=latin |
| DM Sans | 400, 500, 600, 700 | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=DM%20Sans:wght@400;500;600;700&subset=latin |
| Instrument Sans | 400, 500, 600, 700 | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=Instrument%20Sans:wght@400;500;600;700&subset=latin |
| Manrope | 400, 500, 600, 700 | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=Manrope:wght@400;500;600;700&subset=latin |
| Space Grotesk | 400, 500, 600, 700 | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=Space%20Grotesk:wght@400;500;600;700&subset=latin |

## Mono

| Family | Weights downloaded | Licence | Source URL |
| --- | --- | --- | --- |
| JetBrains Mono | 400, 500, 600, 700 | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=JetBrains%20Mono:wght@400;500;600;700&subset=latin |
| IBM Plex Mono | 400, 500, 600, 700 | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=IBM%20Plex%20Mono:wght@400;500;600;700&subset=latin |
| Space Mono | 400, 700 | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=Space%20Mono:wght@400;500;600;700&subset=latin |
| DM Mono | 400, 500 | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=DM%20Mono:wght@400;500;600;700&subset=latin |
| Fira Code | 400, 500, 600, 700 | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=Fira%20Code:wght@400;500;600;700&subset=latin |
| Geist Mono | 400, 500, 600, 700 | OFL-1.1 (`OFL.txt`) | https://fonts.googleapis.com/css2?family=Geist%20Mono:wght@400;500;600;700&subset=latin |

## Arabic fallbacks

Bundled so Arabic renders without any system font (the Latin families above have no Arabic coverage).
Downloaded from the official `github.com/google/fonts` repository on 2026-10-13 as the upstream
variable fonts, then instanced to static per-weight files with `fontTools.varLib.instancer`
(`--update-name-table`; Noto Sans Arabic also pinned to `wdth=100`).

| Family | Weights | Licence | Source URL |
| --- | --- | --- | --- |
| Noto Sans Arabic | 400, 700 | OFL-1.1 (`OFL.txt`) | https://raw.githubusercontent.com/google/fonts/main/ofl/notosansarabic/NotoSansArabic%5Bwdth%2Cwght%5D.ttf |
| Noto Naskh Arabic | 400, 700 | OFL-1.1 (`OFL.txt`) | https://raw.githubusercontent.com/google/fonts/main/ofl/notonaskharabic/NotoNaskhArabic%5Bwght%5D.ttf |

Variable sources (sha256): `NotoSansArabic[wdth,wght].ttf`
`63111b5b2e074dd48cc67692e0a2726d86ee94c1c37fe8598257b7b4e87e869e`; `NotoNaskhArabic[wght].ttf`
`67b5a525a661b607971fbd3f96a81b89d3a768e74534fca84f18ac97e6fab72f`.

Static files (sha256), all `fc-scan`-verified `variable=False` with the named weight:

| File | Bytes | sha256 |
| --- | --- | --- |
| `NotoSansArabic/NotoSansArabic-400.ttf` | 194336 | `f93c562cfa1b722ce739436f020b6fab0db27bf1afa3ddf32e706b6fb7002ded` |
| `NotoSansArabic/NotoSansArabic-700.ttf` | 194520 | `a976fc7c770d9dafff083de450e1b3c86694bee2b1856f80f5e8decfe305c531` |
| `NotoNaskhArabic/NotoNaskhArabic-400.ttf` | 200416 | `bf809c7898e3292ed597912abaada2db376419aa280df95681b86b556aec901e` |
| `NotoNaskhArabic/NotoNaskhArabic-700.ttf` | 200524 | `5d6c235659833cc66d5da21eca6b4b18381a2f64f4c014d88cda8d142695dd52` |

Notes

- GPUI registers the faces via `src/ui/fonts.rs` (`FONTS`), and `fonts::themed` / `fonts::arabic_fallbacks`
  attach `Noto Sans Arabic` then `Noto Naskh Arabic` as `Font.fallbacks` on the input widget. On Linux
  (cosmic-text 0.14 + GPUI 0.2.2) glyph fallback is resolved against the font database, so registration
  is what makes Arabic render; `Font.fallbacks` is not consumed there.
- cosmic-text's Unix script fallback for Arabic names `Noto Sans Arabic` directly, so the bundled sans
  face is selected by name. `Noto Naskh Arabic` is bundled and registered but is not on that list, so it
  is only used where a caller asks for it explicitly (its `Font.fallbacks` slot in `fonts::themed`).

## Notes

- Only weights that exist upstream were requested; the API returns its available weights when a superset is asked for, so 400/500/600/700 is requested for every family and whatever exists lands on disk.
- 400-only families: Pirata One, Italiana, Gloock, Metamorphous, Young Serif, Bebas Neue, Press Start 2P. 400+700 only: Philosopher, Silkscreen, Space Mono. DM Mono stops at 500. UnifrakturCook is 700-only.
- `ZenOldMincho/` is 21 MiB of the total: Zen Old Mincho ships full CJK coverage (kana/kanji) per weight. Subsetting it to latin would cut ~20 MiB if the app never renders Japanese glyphs.
- Licence text for every family is `OFL.txt` (SIL Open Font License 1.1) taken from `github.com/google/fonts/ofl/<family>/OFL.txt`.

## Subsetting

Zen Old Mincho was subset to Latin, punctuation, currency, arrows and math symbols with fontTools `pyftsubset` (21 MiB → 164 KiB); the app only uses it for Latin display text. OFL 1.1 permits modified versions; the family name is unchanged because this is an unmodified-glyph subset for embedding.

Total after subsetting: about 7.5 MiB.
