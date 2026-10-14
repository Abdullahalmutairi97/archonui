// usage: node shot.js <out-prefix> <steps.json>  ; steps: [["click","selector"],["eval","js"],["shot","name"],["wait",ms],["key","Key"],["clickText","text"]]
const { chromium } = require('playwright');
(async () => {
  const [prefix, stepsFile] = process.argv.slice(2);
  const steps = stepsFile ? JSON.parse(require('fs').readFileSync(stepsFile, 'utf8')) : [];
  const browser = await chromium.launch({ channel: 'chrome' });
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  page.on('pageerror', e => console.log('PAGEERROR', e.message));
  await page.goto('file:///home/archonminipc/ArchonUI/design/ArchonUI%20Agent%20v3.dc.html');
  await page.waitForTimeout(2500);
  let n = 0;
  for (const [op, a, b] of steps) {
    if (op === 'click') await page.click(a, { timeout: 5000 }).catch(e => console.log('CLICKFAIL', a, e.message.split('\n')[0]));
    else if (op === 'clickText') await page.getByText(a, { exact: !!b }).first().click({ timeout: 5000 }).catch(e => console.log('CLICKFAIL', a, e.message.split('\n')[0]));
    else if (op === 'clickXY') await page.mouse.click(a, b);
    else if (op === 'key') await page.keyboard.press(a);
    else if (op === 'type') await page.keyboard.type(a);
    else if (op === 'wait') await page.waitForTimeout(a);
    else if (op === 'eval') console.log('EVAL', JSON.stringify(await page.evaluate(a)).slice(0, b || 4000));
    else if (op === 'shot') await page.screenshot({ path: `${prefix}-${a || n++}.png` });
  }
  if (!steps.length) await page.screenshot({ path: `${prefix}-0.png` });
  await browser.close();
})();
