// Renders preview.html to PNG screenshots in screenshots/ (Playwright, Chromium).
import { chromium } from 'playwright-core';
import { mkdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const ROOT = resolve(import.meta.dirname, '..');
const out = join(ROOT, 'screenshots');
mkdirSync(out, { recursive: true });
const browser = await chromium.launch();
try {
  for (const dpr of [1, 2]) {
    const page = await browser.newPage({ viewport: { width: 1600, height: 1000 }, deviceScaleFactor: dpr });
    await page.goto(pathToFileURL(join(ROOT, 'preview.html')).href);
    const sfx = dpr === 1 ? '' : '@2x';
    if (dpr === 1) await page.screenshot({ path: join(out, 'preview-full.png'), fullPage: true });
    const sections = await page.$$('section');
    const names = ['masters-1024', 'family-256', 'small-sizes', 'macos-dock', 'windows-taskbar', 'tray', 'palette-cvd', 'alternative-carbon', 'umbrella-paddock'];
    for (let i = 0; i < sections.length; i++) await sections[i].screenshot({ path: join(out, `${names[i] ?? `section-${i}`}${sfx}.png`) });
    await page.close();
  }
} finally {
  await browser.close();
}
console.log('screenshots written to', out);
