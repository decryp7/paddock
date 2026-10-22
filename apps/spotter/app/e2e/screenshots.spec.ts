import { test } from '@playwright/test';

/** Documentation screenshots (`SCREENSHOTS=1 npx playwright test --project=screenshots`). */
const OUT = '../docs/screenshots';

for (const scheme of ['dark', 'light'] as const) {
  test.describe(scheme, () => {
    test.use({ colorScheme: scheme });
    const shots: [string, string][] = [
      ['dashboard', '/'],
      ['watches', '/#/watches'],
      ['watch-detail', '/#/watches/w-1'],
      ['settings', '/#/settings'],
      ['tray', '/#/tray'],
    ];
    for (const [name, url] of shots) {
      test(`${name} ${scheme}`, async ({ page }) => {
        await page.goto(url);
        await page.waitForTimeout(1200);
        await page.screenshot({ path: `${OUT}/${name}-${scheme}.png` });
      });
    }
    test(`wizard ${scheme}`, async ({ page }) => {
      await page.goto('/#/watches/new');
      await page.waitForTimeout(600);
      await page.screenshot({ path: `${OUT}/wizard-source-${scheme}.png` });
      await page.getByRole('button', { name: /Node\.js/ }).click();
      await page.waitForTimeout(1500);
      await page.screenshot({ path: `${OUT}/wizard-configure-${scheme}.png` });
    });
  });
}
