const { chromium } = require('playwright');
const fs = require('fs');
(async () => {
  const expr = fs.readFileSync('/home/archonminipc/ArchonUI/.qa/design/algo.js', 'utf8').trim();
  const browser = await chromium.launch({ channel: 'chrome' });
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  await page.goto('file:///home/archonminipc/ArchonUI/design/ArchonUI%20Agent%20v3.dc.html');
  await page.waitForTimeout(2500);
  const raw = await page.evaluate(expr);
  const sigils = JSON.parse(raw);
  // cross-check against the six icons actually rendered in the DOM (seed sessions 0,17,34,51,68,85)
  const dom = await page.evaluate(() => Array.from(document.querySelectorAll('svg path')).map(p => p.getAttribute('d')).filter(d => d && /a0\.9 |a1\.9 |h2\.8v2\.8/.test(d)));
  const ids = [0, 17, 34, 51, 68, 85];
  const checks = ids.map(i => ({ i, match: dom.includes(sigils[i].d) }));
  fs.writeFileSync('/home/archonminipc/ArchonUI/.qa/design/sigils.json', JSON.stringify({ sigils, domChecks: checks, domCount: dom.length }, null, 0));
  console.log(JSON.stringify({ count: sigils.length, checks }));
  await browser.close();
})();
