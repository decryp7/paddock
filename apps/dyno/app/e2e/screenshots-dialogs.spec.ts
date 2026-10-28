import { mkdirSync } from 'node:fs';
import { FIXTURES as DIALOG_FIXTURES } from './dialog-fixtures';
import { dialogContracts, dialogWindowUrl, expect, test, type DialogContractEntry } from '@decryp7/paddock-e2e';

// Every Dyno dialog at its fitted size, light and dark, in the macOS and the Windows button order, for the independent
// review (rule 22). Only on request: `SCREENSHOTS=1 DIALOG_SHOTS_DIR=<dir> npx playwright test --project=screenshots e2e/screenshots-dialogs.spec.ts`.
const UI = '../../../ui/projects/paddock-ui/shell';
const CONTRACTS = dialogContracts('src/app/generated/dialog-contracts.ts', ...['about', 'confirm'].map((p) => `${UI}/${p}/generated/dialog-contracts.ts`));
const OUT = process.env['DIALOG_SHOTS_DIR'] ?? '';
const PLATFORMS = { mac: { platform: 'MacIntel', ua: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 Chrome/130 Safari/537.36' }, win: { platform: 'Win32', ua: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/130 Safari/537.36' } } as const;
const FIXTURES: Record<string, unknown> = Object.fromEntries(Object.entries(DIALOG_FIXTURES).map(([id, list]) => [id, list[0]]));
const CAP = 0.75;

test.describe('Dyno dialog screenshots', () => {
  test.skip(!OUT, 'set DIALOG_SHOTS_DIR');
  for (const [id, contract] of Object.entries(CONTRACTS) as [string, DialogContractEntry][]) {
    for (const scheme of ['light', 'dark'] as const) {
      for (const [os, p] of Object.entries(PLATFORMS)) {
        test(`${id} ${scheme} ${os}`, async ({ browser, baseURL }) => {
          mkdirSync(OUT, { recursive: true });
          const area = { width: 1920, height: 1080 };
          const capH = Math.floor(area.height * CAP);
          const context = await browser.newContext({ baseURL, colorScheme: scheme, deviceScaleFactor: 2, userAgent: p.ua, viewport: { width: contract.width, height: capH } });
          await context.addInitScript((platform) => Object.defineProperty(navigator, 'platform', { get: () => platform }), p.platform);
          try {
            const page = await context.newPage();
            await page.goto(dialogWindowUrl(id, FIXTURES[id] ?? {}, { title: id }));
            await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
            const asked = await page.evaluate(() => ({ h: Number(document.documentElement.dataset['fittedHeight'] ?? '0'), min: document.documentElement.dataset['minContent'] ?? '0x0' }));
            const minH = Number(asked.min.split('x')[1] ?? '0');
            const height = contract.kind === 'tool' ? Math.min(contract.height ?? capH, capH) : Math.max(Math.min(asked.h, capH), minH);
            await page.setViewportSize({ width: contract.width, height });
            await expect(page.locator('[data-testid=dialog-footer]')).toBeVisible();
            await page.screenshot({ path: `${OUT}/${id}--${scheme}--${os}.png` });
          } finally {
            await context.close();
          }
        });
      }
    }
  }
});
