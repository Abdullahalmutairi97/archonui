# Main Window UI Diff Review

## 1. Main Launch / Default State
- **Image Pair**: `.qa/review/main-00-launch.png` vs `.qa/design/layout-default.png`
- **Differences**:
  1. (Medium) Sidebar logo placement or spacing offset inside brand header (2px difference in top margin). File: `src/ui/sidebar.rs`.
  2. (Low) Composer shadow elevation blur radius (1px narrower blur radius). File: `src/ui/composer.rs`.

## 2. Session View & Stream Header
- **Image Pair**: `.qa/review/main-01-session.png` vs `.qa/design/layout-default.png`
- **Differences**:
  1. (Medium) Tile header action button padding (horizontal padding 6px vs prototype 8px). File: `src/ui/tiles.rs`.
  2. (Low) Agent session pill font size (11.5px vs 12px design spec). File: `src/ui/tiles.rs`.

## 3. 3 Tiles Layout (`Alt+Return`)
- **Image Pair**: `.qa/review/main-03-tiles3.png` vs `.qa/design/layout-split.png`
- **Differences**:
  1. (Medium) Split divider handle thickness (4px hit target vs 2px visible stroke). File: `src/ui/tiling.rs`.
  2. (Low) Focused tile border glow intensity (1px accent border vs prototype shadow inset). File: `src/ui/tiles.rs`.

## 4. 5 Tiles Layout (`Alt+Q`)
- **Image Pair**: `.qa/review/main-04-tiles5.png` vs `.qa/review/proto-main-tiles5b.png`
- **Differences**:
  1. (Low) Multi-tile grid gap spacing (7px vs prototype 8px gap). File: `src/ui/tiling.rs`.

## 5. Empty Tile State
- **Image Pair**: `.qa/review/main-05-empty-tile.png` vs `.qa/review/proto-main-empty-tile.png`
- **Differences**:
  1. (Low) Empty state prompt icon centering vertical offset (3px higher). File: `src/ui/tiles.rs`.

## 6. Right Panel - Browser Tab
- **Image Pair**: `.qa/review/main-13-rpanel-browser.png` vs prototype browser tab
- **Differences**:
  1. (Medium) Browser address bar corner radius (6px vs prototype 8px). File: `src/ui/rpanel/browser.rs`.

## 7. Right Panel - Files Tab
- **Image Pair**: `.qa/review/main-14-rpanel-files.png` vs prototype files tab
- **Differences**:
  1. (Low) Tree view indentation spacing per level (14px vs 16px). File: `src/ui/rpanel/files.rs`.

## 8. Right Panel - Git Tab
- **Image Pair**: `.qa/review/main-16-rpanel-git.png` vs prototype git tab
- **Differences**:
  1. (Low) Diff line number column width and gutter padding. File: `src/ui/rpanel/git.rs`.

## 9. Right Panel - Terminal Tab
- **Image Pair**: `.qa/review/main-17-rpanel-terminal.png` vs prototype terminal tab
- **Differences**:
  1. (Low) Terminal background contrast tint (2% darker alpha). File: `src/ui/rpanel/terminal.rs`.

## 10. Stream Blocks & Agent Response
- **Image Pair**: `.qa/review/main-20-blocks.png` vs prototype stream blocks
- **Differences**:
  1. (High) Tool call row collapse/expand icon alignment and border padding. File: `src/ui/stream/tool.rs`.
  2. (Medium) Code block header copy button margin (4px offset). File: `src/ui/stream/code.rs`.

---
**High Severity Items Summary**:
- Stream Tool Calls: Tool call row collapse/expand icon alignment and border padding (`src/ui/stream/tool.rs`).
