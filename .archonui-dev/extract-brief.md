# Shared brief for design-extraction workers (read-only on code)

Project: ArchonUI, a GPUI (Rust) desktop app at /home/archonminipc/ArchonUI. We are porting a new UI from an HTML prototype:
`/home/archonminipc/ArchonUI/design/ArchonUI Agent v3.dc.html` (1689 lines; uses `./support.js` runtime; assets in `design/assets/`).
The prototype is the exact visual + behavioural spec. Your job is to EXTRACT exact values into a markdown/JSON doc so Rust developers can port it without reading the HTML. Never guess: lift values verbatim from the source (hex colours, px, rgba alphas, font stacks, durations, easing curves, text labels). When a value is computed in JS, give the formula and the computed values.

Rules
- Do NOT edit anything outside your one output file (named in your task). Do not touch src/.
- You can render the prototype and click through it with Playwright:
  `cd /home/archonminipc/ArchonUI/.archonui-dev && NODE_PATH=/home/archonminipc/.local/lib/node_modules node shot.js /home/archonminipc/ArchonUI/.qa/design/<yourname> steps.json`
  where steps.json (write it under /home/archonminipc/ArchonUI/.qa/design/) is a JSON array of steps:
  ["click", css], ["clickText", text, exact?], ["clickXY", x, y], ["key", "Escape"], ["type", "text"], ["wait", ms], ["shot", "name"], ["eval", "js expression returning JSON-able", maxChars].
  Viewport is 1600x1000. Use ["eval", ...] to read computed styles: e.g. `getComputedStyle(document.querySelector(sel)).backgroundColor`.
  A reference screenshot of the default state: /home/archonminipc/ArchonUI/.qa/design/default.png.
- Prefer reading the source (grep/sed with line numbers) and use the browser to confirm/compute values and to see interactions.
- Output: a precise, well-structured markdown file (tables where useful) or JSON, with source line references (e.g. `L512`) for every value group.
- Finish with a short report (under 15 lines): what you extracted, anything ambiguous.
