const T = (wall, render, bg, panel, pop, fg, muted, faint, line, line2, acc, acc2, deep, alt, on, soft, logo) => ({ wall: 'assets/fhd/' + wall, render, bg, panel, pop, fg, muted, faint, line, line2, acc, acc2, deep, alt, on, soft, logo: 'assets/' + logo, border: `linear-gradient(135deg,${acc2},${acc} 50%,${deep})` });
const THEMES = {
  ArchonUI: T('archonui4.jpg', 'auto', '#050505', 'rgba(8,8,8,0.80)', 'rgba(14,14,14,0.97)', '#ededed', '#8a8a8a', '#3a3a3a', 'rgba(237,237,237,0.06)', 'rgba(237,237,237,0.15)', '#d8d8d8', '#ffffff', '#4a4a4a', '#b0b0b0', '#050505', 'rgba(255,255,255,0.12)', 'sword-logo-diag.png'),
  'Blood Moon': T('bloodmoon2.jpg', 'auto', '#0d0a08', 'rgba(13,10,8,0.80)', 'rgba(18,13,10,0.97)', '#eadfcc', '#9a8c78', '#43382c', 'rgba(234,223,204,0.06)', 'rgba(234,223,204,0.15)', '#b0141e', '#d9b56a', '#5a0a10', '#c9a77a', '#f6ecdc', 'rgba(176,20,30,0.24)', 'logo-bm.png'),
  Moonlight: T('moonlight2.jpg', 'auto', '#050a14', 'rgba(6,12,24,0.72)', 'rgba(8,14,28,0.97)', '#e3ecf8', '#8392aa', '#2c3a52', 'rgba(227,236,248,0.06)', 'rgba(227,236,248,0.16)', '#8fb8e8', '#dceaff', '#33557f', '#b4c6e0', '#06101f', 'rgba(143,184,232,0.20)', 'logo-moon.png'),
  Wreck: T('wreck2.jpg', 'auto', '#1e140a', 'rgba(30,20,10,0.78)', 'rgba(34,23,12,0.97)', '#f3e6cc', '#a8916c', '#5a4428', 'rgba(243,230,204,0.06)', 'rgba(243,230,204,0.15)', '#c8893a', '#f0c27a', '#6b4420', '#e3cfa4', '#1e140a', 'rgba(200,137,58,0.22)', 'logo-sand.png'),
  Citadel: T('citadel2.jpg', 'auto', '#0d0f18', 'rgba(13,15,24,0.76)', 'rgba(16,18,28,0.97)', '#f2e6d0', '#8f8ea4', '#383a52', 'rgba(242,230,208,0.06)', 'rgba(242,230,208,0.15)', '#e09a3a', '#ffcc70', '#7a3d14', '#9fb0d8', '#140c04', 'rgba(224,154,58,0.22)', 'logo-drg-black.png'),
  Neon: T('neon2.jpg', 'auto', '#0e0718', 'rgba(14,7,24,0.74)', 'rgba(18,9,30,0.97)', '#f1e6ff', '#9d8bb8', '#3d2a5a', 'rgba(241,230,255,0.06)', 'rgba(241,230,255,0.15)', '#b04cff', '#ff6ad5', '#4b1d7a', '#93a8ff', '#ffffff', 'rgba(176,76,255,0.24)', 'logo-cat.png'),
  Violet: T('violet2.jpg', 'auto', '#0c0a20', 'rgba(12,10,32,0.74)', 'rgba(15,13,38,0.97)', '#ece8ff', '#9590c0', '#36316a', 'rgba(236,232,255,0.06)', 'rgba(236,232,255,0.15)', '#7a6cff', '#c9b8ff', '#33287a', '#e08ab8', '#ffffff', 'rgba(122,108,255,0.24)', 'logo-portal.png'),
  Ink: T('ink2.jpg', 'auto', '#f2f0ea', 'rgba(246,244,238,0.86)', 'rgba(250,248,243,0.98)', '#16130f', '#6b665d', '#bdb8ad', 'rgba(22,19,15,0.06)', 'rgba(22,19,15,0.15)', '#c8201e', '#e0322a', '#16130f', '#3d3a35', '#ffffff', 'rgba(200,32,30,0.12)', 'logo-tree.png'),
  Bonfire: T('bonfire.jpg', 'pixelated', '#141b20', 'rgba(20,28,34,0.80)', 'rgba(24,32,38,0.97)', '#dbe6ea', '#8a9ba3', '#3a4a52', 'rgba(219,230,234,0.06)', 'rgba(219,230,234,0.16)', '#e8762c', '#ffb24a', '#8a3a1c', '#9cc8d2', '#1a0f08', 'rgba(232,118,44,0.22)', 'logo-bonfire.png'),
  Hunter: T('hunter2.jpg', 'pixelated', '#0f1622', 'rgba(15,22,34,0.80)', 'rgba(18,26,40,0.97)', '#e4e8f0', '#8794ad', '#323d55', 'rgba(228,232,240,0.06)', 'rgba(228,232,240,0.15)', '#c23a3a', '#f0e2c0', '#6e1e24', '#8fa6cc', '#0f1622', 'rgba(194,58,58,0.24)', 'logo-helm.png'),
  Shards: T('shards2.jpg', 'auto', '#0a0a0c', 'rgba(10,10,12,0.78)', 'rgba(14,14,17,0.97)', '#eef0f4', '#8c92a0', '#353944', 'rgba(238,240,244,0.06)', 'rgba(238,240,244,0.15)', '#9aa4b8', '#ffffff', '#3a3f4c', '#c2c8d6', '#0a0a0c', 'rgba(154,164,184,0.20)', 'logo-snow.png'),
  Vale: T('vale2.jpg', 'auto', '#0d150c', 'rgba(13,21,12,0.76)', 'rgba(16,25,15,0.97)', '#eef2e0', '#94a283', '#34442c', 'rgba(238,242,224,0.06)', 'rgba(238,242,224,0.15)', '#d0882e', '#f2c36a', '#5a3a14', '#8cc06a', '#140c04', 'rgba(208,136,46,0.22)', 'logo-galaxy.png'),
};
const NAMES = Object.keys(THEMES);
const JB = "'JetBrains Mono', monospace", PLEX = "'IBM Plex Mono', monospace", SPACE = "'Space Mono', monospace", DMM = "'DM Mono', monospace", FIRA = "'Fira Code', monospace";
const F = (n, d, u) => ({ n, d, u });
const FONTS = {
  ArchonUI: [F('Grenze', "'Grenze Gotisch', serif", PLEX), F('Bodoni', "'Bodoni Moda', serif", JB), F('Italiana', "'Italiana', serif", DMM)],
  'Blood Moon': [F('Fraktur', "'UnifrakturCook', serif", JB), F('Pirata', "'Pirata One', serif", SPACE), F('Grenze', "'Grenze Gotisch', serif", PLEX)],
  Moonlight: [F('Cormorant', "'Cormorant Garamond', serif", JB), F('Italiana', "'Italiana', serif", DMM), F('Bodoni', "'Bodoni Moda', serif", PLEX)],
  Wreck: [F('Bebas', "'Bebas Neue', sans-serif", SPACE), F('Orbitron', "'Orbitron', sans-serif", JB), F('Gloock', "'Gloock', serif", DMM)],
  Citadel: [F('Grenze', "'Grenze Gotisch', serif", JB), F('Cinzel', 'Cinzel, serif', PLEX), F('Fraktur', "'UnifrakturCook', serif", SPACE)],
  Neon: [F('Orbitron', "'Orbitron', sans-serif", JB), F('Mincho', "'Zen Old Mincho', serif", DMM), F('Bebas', "'Bebas Neue', sans-serif", FIRA)],
  Violet: [F('Cormorant', "'Cormorant Garamond', serif", JB), F('Metamorphous', "'Metamorphous', serif", DMM), F('Cinzel', 'Cinzel, serif', PLEX)],
  Ink: [F('Mincho', "'Zen Old Mincho', serif", DMM), F('Cormorant', "'Cormorant Garamond', serif", JB), F('Bodoni', "'Bodoni Moda', serif", PLEX)],
  Bonfire: [F('Pixelify', "'Pixelify Sans', monospace", JB), F('Silkscreen', "'Silkscreen', monospace", SPACE), F('Press Start', "'Press Start 2P', monospace", PLEX)],
  Hunter: [F('Silkscreen', "'Silkscreen', monospace", JB), F('Pixelify', "'Pixelify Sans', monospace", SPACE), F('Press Start', "'Press Start 2P', monospace", PLEX)],
  Shards: [F('Orbitron', "'Orbitron', sans-serif", SPACE), F('Bebas', "'Bebas Neue', sans-serif", JB), F('Italiana', "'Italiana', serif", DMM)],
  Vale: [F('Metamorphous', "'Metamorphous', serif", JB), F('Young Serif', "'Young Serif', serif", DMM), F('Philosopher', "'Philosopher', sans-serif", FIRA)],
};
const DISPLAY = [['Fraktur', "'UnifrakturCook', serif"], ['Pirata', "'Pirata One', serif"], ['Grenze', "'Grenze Gotisch', serif"], ['Cinzel', 'Cinzel, serif'], ['Cormorant', "'Cormorant Garamond', serif"], ['Italiana', "'Italiana', serif"], ['Bodoni', "'Bodoni Moda', serif"], ['Gloock', "'Gloock', serif"], ['Mincho', "'Zen Old Mincho', serif"], ['Metamorphous', "'Metamorphous', serif"], ['Young Serif', "'Young Serif', serif"], ['Philosopher', "'Philosopher', sans-serif"], ['Bebas', "'Bebas Neue', sans-serif"], ['Orbitron', "'Orbitron', sans-serif"], ['Pixelify', "'Pixelify Sans', monospace"], ['Silkscreen', "'Silkscreen', monospace"], ['Press Start', "'Press Start 2P', monospace"]];
const MONO = [['JetBrains Mono', JB], ['IBM Plex Mono', PLEX], ['Space Mono', SPACE], ['DM Mono', DMM], ['Fira Code', FIRA], ['Geist Mono', "'Geist Mono', monospace"], ['Geist', "'Geist', sans-serif"], ['IBM Plex Sans', "'IBM Plex Sans', sans-serif"], ['DM Sans', "'DM Sans', sans-serif"], ['Instrument Sans', "'Instrument Sans', sans-serif"], ['Manrope', "'Manrope', sans-serif"], ['Space Grotesk', "'Space Grotesk', sans-serif"]];
const SANS_D = [['Geist', "'Geist', sans-serif"], ['Plex Sans', "'IBM Plex Sans', sans-serif"], ['DM Sans', "'DM Sans', sans-serif"], ['Instrument', "'Instrument Sans', sans-serif"], ['Manrope', "'Manrope', sans-serif"], ['Grotesk', "'Space Grotesk', sans-serif"]];
DISPLAY.unshift(...SANS_D);
const MIN_PAIRS = [['Geist', "'Geist', sans-serif", "'Geist', sans-serif"], ['IBM Plex', "'IBM Plex Sans', sans-serif", "'IBM Plex Sans', sans-serif"], ['DM Sans', "'DM Sans', sans-serif", "'DM Sans', sans-serif"], ['Instrument', "'Instrument Sans', sans-serif", "'Geist Mono', monospace"], ['Manrope', "'Manrope', sans-serif", JB], ['Grotesk', "'Space Grotesk', sans-serif", SPACE]];
const FENCE = new RegExp('\x60{3}([^\n]*)\n([\\s\\S]*?)\x60{3}', 'g');
const rgbaOf = (c, a) => 'rgba(' + [1, 3, 5].map(i => parseInt(c.slice(i, i + 2), 16)).join(',') + ',' + a + ')';

// ---- extraction harness ----
const hx = c => [1,3,5].map(i => parseInt(c.slice(i,i+2),16));
const pad = v => Math.round(v).toString(16).padStart(2,'0');
const mixHex = (a,b,k) => '#' + hx(a).map((v,i)=> pad(v + (hx(b)[i]-v)*k)).join('');
const rgbaH = (c,a) => 'rgba(' + hx(c).join(',') + ',' + a + ')';
const out = [];
for (const name of NAMES) {
  const base = THEMES[name];
  const TUNE = name === 'Blood Moon'
    ? { tile:'#16100d', panel:'#0f0b09', pop:'#1b1411', fg:'#f3ead8', muted:'#bfae94', faint:'#6a5a48', soft:'rgba(176,20,30,0.32)', acc:'#c81e2a' }
    : { tile: mixHex(base.bg, base.fg, 0.05), panel: mixHex(base.bg, base.fg, 0.02), pop: mixHex(base.bg, base.fg, 0.08), muted: mixHex(base.muted, base.fg, 0.3), faint: mixHex(base.faint, base.fg, 0.12) };
  const t0 = { ...base, ...TUNE };
  const derived = {
    line: rgbaH(t0.fg, 0.09),
    line2: rgbaH(t0.fg, 0.22),
    border: 'linear-gradient(135deg,' + t0.acc2 + ',' + t0.acc + ' 50%,' + t0.deep + ')',
  };
  const bands = [0,1,2,3,4,5].map(i => ({
    hex: mixHex(t0.acc, t0.acc2, (i%6)*0.07),
    rgba: (a) => 'rgba(' + hx(mixHex(t0.acc,t0.acc2,(i%6)*0.07)).join(',') + ',' + a + ')',
    bandRgba: 'rgba(' + hx(mixHex(t0.acc,t0.acc2,(i%6)*0.07)).join(',') + ',' + (i===0? '0.42/0.20' : '0.42/0.20') + ')'
  }));
  out.push({ name, base: { wall: base.wall, render: base.render, bg: base.bg, panel: base.panel, pop: base.pop, fg: base.fg, muted: base.muted, faint: base.faint, line: base.line, line2: base.line2, acc: base.acc, acc2: base.acc2, deep: base.deep, alt: base.alt, on: base.on, soft: base.soft, logo: base.logo, border: base.border }, tuned: { ...t0 }, derived,
    bands: bands.map(b => ({ hex: b.hex, alphaFocused: 'rgba('+hx(b.hex).join(',')+',0.42)', alphaNormal: 'rgba('+hx(b.hex).join(',')+',0.20)' })),
    fonts: FONTS[name] });
}
console.log(JSON.stringify({ out, DISPLAY, MONO, SANS_D, MIN_PAIRS, NAMES }, null, 1));
