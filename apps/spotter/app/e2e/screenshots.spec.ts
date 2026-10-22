import { test, type Page } from '@playwright/test';

/** Documentation screenshots (`SCREENSHOTS=1 npx playwright test --project=screenshots`): 1440x900 at 2x, both themes. */
const OUT = process.env['SHOTS_OUT'] ?? '../docs/screenshots';

async function settle(page: Page, ms = 900): Promise<void> {
  await page.waitForTimeout(ms);
}

for (const scheme of ['dark', 'light'] as const) {
  test.describe(scheme, () => {
    test.use({ colorScheme: scheme });
    const shots: [string, string][] = [
      ['dashboard', '/'],
      ['watches', '/#/watches'],
      ['watch-detail', '/#/watches/w-1'],
      ['watch-detail-products', '/?mock=products#/watches/w-9'],
      ['dashboard-empty', '/?mock=empty'],
      ['watches-empty', '/?mock=empty#/watches'],
      ['settings', '/#/settings'],
    ];
    for (const [name, url] of shots) {
      test(`${name} ${scheme}`, async ({ page }) => {
        await page.goto(url);
        await settle(page, 1400);
        await page.screenshot({ path: `${OUT}/${name}-${scheme}.png` });
        // Long pages: the page scrolls inside the shell, so shoot the part below the fold too.
        const more = await page.evaluate(() => {
          const el = [...document.querySelectorAll('*')].find((e) => e.scrollHeight > e.clientHeight + 40 && getComputedStyle(e).overflowY !== 'visible');
          if (!el) return false;
          el.scrollTop = el.scrollHeight;
          return true;
        });
        if (more) {
          await settle(page, 300);
          await page.screenshot({ path: `${OUT}/${name}-2-${scheme}.png` });
        }
      });
    }
    test(`cadence ${scheme}`, async ({ page }) => {
      await page.goto('/');
      await settle(page, 1400);
      await page.locator('#cad-h').scrollIntoViewIfNeeded();
      await page.locator('#cad-h').evaluate((e) => e.scrollIntoView({ block: 'start' }));
      await settle(page, 400);
      await page.screenshot({ path: `${OUT}/dashboard-cadence-${scheme}.png` });
    });
    test(`release panel ${scheme}`, async ({ page }) => {
      await page.goto('/#/watches/w-2');
      await settle(page, 1200);
      await page.locator('tbody tr').nth(1).click();
      await settle(page, 400);
      await page.screenshot({ path: `${OUT}/watch-detail-panel-${scheme}.png` });
    });
    test(`tray ${scheme}`, async ({ page }) => {
      await page.setViewportSize({ width: 372, height: 500 });
      await page.goto('/#/tray');
      await settle(page, 1200);
      await page.screenshot({ path: `${OUT}/tray-${scheme}.png` });
    });
    test(`schedule ${scheme}`, async ({ page }) => {
      await page.goto('/#/watches/new');
      await page.getByRole('button', { name: /Node\.js/ }).click();
      await settle(page, 800);
      await page.getByRole('button', { name: 'Next' }).click();
      await page.getByRole('button', { name: 'Next' }).click();
      await page.getByRole('group', { name: 'How to schedule checks' }).getByRole('button', { name: 'Cron' }).click();
      await page.getByLabel('Cron expression').fill('0 9,14 * * 1-5');
      await settle(page, 1200);
      await page.screenshot({ path: `${OUT}/wizard-schedule-${scheme}.png` });
    });
    test(`wizard ${scheme}`, async ({ page }) => {
      await page.goto('/#/watches/new');
      await settle(page, 700);
      await page.screenshot({ path: `${OUT}/wizard-source-${scheme}.png` });
      await page.getByRole('button', { name: /Node\.js/ }).click();
      await settle(page, 1600);
      await page.screenshot({ path: `${OUT}/wizard-configure-${scheme}.png` });
    });
  });
}
