#!/usr/bin/env node
// Renders the Environment mock-up (ADR 0022): compiles the family tokens from ui/projects/paddock-ui/scss (so the mock-up
// always shows the current tokens and no compiled copy is committed), copies the page next to them in a temporary
// folder, and shoots every <figure id="f-..."> at 2x in the light and dark schemes into renders/.
//
//   npm ci                      # once, at the repository root (sass and @playwright/test come from the ui workspace)
//   node apps/pitcrew/docs/design/render.mjs [--open]
//
// --open prints the temporary page's path instead of shooting, to look at it in a browser. Uses Playwright's own pinned
// Chromium, never an installed browser (rule 7). PADDOCK_NODE_MODULES may point at another checkout's node_modules.
import { createRequire } from 'node:module';
import { copyFileSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '../../../..');
const require = createRequire(process.env.PADDOCK_NODE_MODULES ? join(process.env.PADDOCK_NODE_MODULES, 'x.js') : join(root, 'ui/package.json'));
const sass = require('sass');
const { chromium } = require('@playwright/test');

const work = mkdtempSync(join(tmpdir(), 'pitcrew-env-mock-'));
const css = sass.compile(join(root, 'ui/projects/paddock-ui/scss/entry-tokens.scss'), { style: 'expanded' }).css;
writeFileSync(join(work, 'tokens.generated.css'), css);
copyFileSync(join(here, 'environment.html'), join(work, 'environment.html'));
const page = join(work, 'environment.html');

if (process.argv.includes('--open')) {
  console.log(page);
} else {
  const out = join(here, 'renders');
  mkdirSync(out, { recursive: true });
  const browser = await chromium.launch();
  try {
    for (const theme of ['light', 'dark']) {
      const p = await browser.newPage({ viewport: { width: 1400, height: 1000 }, deviceScaleFactor: 2 });
      await p.goto(`file://${page}?theme=${theme}`);
      const ids = await p.$$eval('figure[id^="f-"]', (fs) => fs.map((f) => f.id));
      for (const id of ids) await p.locator(`#${id}`).screenshot({ path: join(out, `${id.slice(2)}-${theme}.png`) });
      await p.close();
    }
  } finally {
    await browser.close();
  }
  console.log(`rendered into ${out}`);
}
