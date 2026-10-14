const fs = require("fs");
const data = JSON.parse(fs.readFileSync("/home/archonminipc/ArchonUI/.qa/design/sigils.json", "utf8"));
const G = [3, 8, 13];
const pts = []; G.forEach(y => G.forEach(x => pts.push([x, y])));
const patterns = data.sigils.map(s => {
  const cells = s.path.map(i => pts[i]);
  const bitmap = [[0, 0, 0], [0, 0, 0], [0, 0, 0]];
  s.path.forEach(i => { bitmap[Math.floor(i / 3)][i % 3] = 1; });
  return { index: s.i, path: s.path, cells, bitmap: bitmap.map(r => r.join("")), len: s.len, variant: s.v, closed: s.closed, d: s.d };
});
const out = {
  _source: "design/ArchonUI Agent v3.dc.html",
  _note: "The 100 session icons are not bitmap dot-grids: each is an SVG path (a sigil) traced through a 3x3 lattice, with stroked circle/square end-markers. Generated deterministically by a seeded PRNG at L1164-L1184. All 100 final path strings below were produced by running the exact generator in the browser (Playwright) and verified against the paths rendered in the DOM (seed sessions 0,17,34,51,68,85 all matched).",
  signature: { startLine: 1164, endLine: 1184, constName: "SIGILS" },
  lattice: { values: [3, 8, 13], count: 9, spacing: 5, viewBox: "0 0 16 16", points: pts, indexFormula: "idx = row*3 + col; x = 3 + 5*col; y = 3 + 5*row" },
  algorithm: {
    prng: "Lehmer / Park-Miller minimal standard: seed=7; rnd = () => (seed = (seed*16807) % 2147483647) / 2147483647",
    steps: [
      "G=[3,8,13]; pts=[]; for y in G: for x in G: pts.push([x,y])  // 9 lattice points",
      "let seed=7; rnd as above",
      "out=[]; seen=Set()",
      "while out.length < 100:",
      "  len = 3 + floor(rnd()*3)            // 3, 4 or 5 points",
      "  path = [floor(rnd()*9)]              // first lattice point",
      "  while path.length < len: n=floor(rnd()*9); if n not in path: path.push(n)   // distinct points",
      "  k = path.join('')",
      "  if seen.has(k) || seen.has(reverse(path).join('')): continue   // dedupe path and its mirror",
      "  seen.add(k)",
      "  [sx,sy]=pts[path[0]]; [ex,ey]=pts[path[len-1]]",
      "  v = out.length % 5; closed = (len > 3 && v === 1)",
      "  d = 'M' + path.map(i=>pts[i].join(' ')).join('L') + (closed?'Z':'')",
      "  d += v===2 ? circle(sx,sy,1.9) : v===3 ? square(sx,sy) : circle(sx,sy,0.9)",
      "  if v===4: d += circle(ex,ey,0.9)",
      "  if v===0 && rnd()>0.5: m=pts[path[1]]; d += circle(m[0],m[1],1.6)",
      "  out.push(d)"
    ],
    markers: {
      circle: "(x,y,r) => M{x-r} {y} a{r} {r} 0 1 0 {2r} 0 a{r} {r} 0 1 0 {-2r} 0   // full circle subpath (stroked, fill:none => ring)",
      square: "(x,y) => M{x-1.4} {y-1.4} h2.8 v2.8 h-2.8 z   // 2.8x2.8 square",
      rules: "v=idx%5: 0 => r0.9 circle at start (+ optional r1.6 circle at path[1] if rnd()>0.5); 1 => r0.9 circle at start (closed path if len>3); 2 => r1.9 circle at start; 3 => 2.8 square at start; 4 => r0.9 circle at start AND r0.9 circle at end."
    },
    prngConsumption: "rnd() is called: once per len, once for path[0], once per rejected/added candidate point, plus (for v===0 candidates only) once for the middle-dot coin flip. The coin-flip draw happens in sequence and shifts all later patterns."
  },
  rendering: {
    element: "<svg width=\"W\" height=\"W\" viewBox=\"0 0 16 16\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.5\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><path d=\"{sigil}\"></path></svg>",
    stroke: "currentColor, width 1.5 (viewBox units), fill:none, round cap+join. NB: markers are stroked outlines (rings/squares), NOT filled dots.",
    colorByContext: "color = focused ? theme.acc2 : open ? theme.fg : theme.muted  (tile header: focused ? acc2 : fg). No per-dot opacity; single currentColor.",
    statusDot: "8x8 circle, border-radius:50%, background = running?acc2 : done?alt : faint, border:2px solid panel/tile",
    computedProof: "Blood Moon default: focused #d9b56a (acc2), open #f3ead8 (fg/TUNE), unfocused #bfae94 (muted/TUNE). getComputedStyle color matched rgb(217,181,106)/rgb(243,234,216)/rgb(191,174,148)."
  },
  assignment: {
    initialSeed: "state.sessions = seed().map((x,i)=>({...x, icon:[0,17,34,51,68,85][i]}))   // L1191 - 6 sessions get spread-out sigils",
    newSession: "const used=new Set(sessions.map(x=>x.icon)); let icon=SIGILS.findIndex((_,k)=>!used.has(k)); if(icon<0) icon=sessions.length % SIGILS.length;   // L1273 - first unused index, else round-robin"
  },
  renderSites: [
    { id: "sidebar-expanded-row", htmlLines: "L70", container: "20x20 grid span (position:relative)", svg: "16x16, stroke-width 1.5", color: "foc?acc2 : open?fg : muted", notes: "status dot at right:-2 bottom:-2" },
    { id: "sidebar-collapsed-rail", htmlLines: "L36", container: "36x36 button, border-radius 9px, display:grid place-items:center", svg: "17x17, stroke-width 1.5", color: "foc?acc2 : open?fg : muted", notes: "button background = rgba(mix(acc,acc2,(i%6)*0.07), foc?0.42:0.2)  (the accent band); status dot at right:1 bottom:1, border 2px t.panel" },
    { id: "tile-header", htmlLines: "L121", container: "20x20 grid span", svg: "16x16, stroke-width 1.5", color: "foc?acc2 : fg", notes: "status dot at right:-2 bottom:-2, border 2px t.tile" }
  ],
  logoWordmark: {
    text: "appName (default 'archonui', editable, maxlength 18)",
    font: "theme.display = s.dispFont || FONTS[theme][fontIdx].d  (per-theme display font)",
    defaultTheme: "Blood Moon => 'UnifrakturCook', serif",
    occurrences: [
      { where: "Sidebar header (expanded)", htmlLines: "L47-L48", logo: "22x22 div, background url(t.logo) center/contain", wordmark: "24px, line-height:1" },
      { where: "Empty workspace (no tiles)", htmlLines: "L111-L112", logo: "72x72, opacity .8, image-rendering t.render", wordmark: "56px, text-shadow 0 2px 24px rgba(0,0,0,.6)" },
      { where: "Tile empty state", htmlLines: "L133-L134", logo: "44x44, opacity .55", wordmark: "28px: 'What should {appName} work on?'" },
      { where: "Login screen", htmlLines: "L366/L369", wordmark: "nav 'archonui' 18px; heading 'Sign in to continue' 36px, text-shadow 0 2px 18px rgba(0,0,0,.7)" },
      { where: "Settings > Appearance IDENTITY name input", htmlLines: "L616/L619", wordmark: "input 34px; Sidebar preview 22px" }
    ]
  },
  identityPresetIcons: {
    location: "Settings > Appearance > IDENTITY > Icon  (task brief said General; source places it under Appearance, pg.appearance)",
    grid: "repeat(7, minmax(0,1fr)), gap 8px; each cell aspect-ratio:1, border-radius 8px, image inside at 58% x 58%",
    sourceLine: "L628 (template) / L1428 (icons list)",
    list: "[...new Set(NAMES.map(nm => THEMES[nm].logo)), ...uploadedIcons]  - 12 theme logos + any user uploads",
    items: [
      { theme: "ArchonUI", file: "assets/sword-logo-diag.png" },
      { theme: "Blood Moon", file: "assets/logo-bm.png" },
      { theme: "Moonlight", file: "assets/logo-moon.png" },
      { theme: "Wreck", file: "assets/logo-sand.png" },
      { theme: "Citadel", file: "assets/logo-drg-black.png" },
      { theme: "Neon", file: "assets/logo-cat.png" },
      { theme: "Violet", file: "assets/logo-portal.png" },
      { theme: "Ink", file: "assets/logo-tree.png" },
      { theme: "Bonfire", file: "assets/logo-bonfire.png" },
      { theme: "Hunter", file: "assets/logo-helm.png" },
      { theme: "Shards", file: "assets/logo-snow.png" },
      { theme: "Vale", file: "assets/logo-galaxy.png" }
    ],
    behavior: "pick sets state.logo; selected cell: outline 2px theme.acc, bg theme.soft, else outline transparent, bg theme.line. 'Use theme icon' resets (opacity .4 when already default). '+' label opens a file input (image/*) -> URL.createObjectURL -> appended to upIcons and set as logo."
  },
  patterns: patterns
};
fs.mkdirSync("/home/archonminipc/ArchonUI/docs/design", { recursive: true });
fs.writeFileSync("/home/archonminipc/ArchonUI/docs/design/icons.json", JSON.stringify(out, null, 2));
console.log("wrote icons.json", fs.statSync("/home/archonminipc/ArchonUI/docs/design/icons.json").size, "bytes;", patterns.length, "patterns");
