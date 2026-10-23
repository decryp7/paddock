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
      ['watch-detail-upgrade', '/#/watches/w-6'],
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
    test(`settings e-mail ${scheme}`, async ({ page }) => {
      await page.goto('/#/settings');
      await settle(page, 1000);
      const mail = page.locator('sp-mail-settings');
      await mail.getByLabel('Server').fill('smtp.example.org');
      await mail.getByLabel('From').fill('spotter@example.org');
      await mail.getByLabel('User name').fill('spotter-robot');
      await mail.getByLabel('Password').fill('example-password');
      await mail.getByRole('button', { name: 'Save' }).click();
      await mail.getByLabel('Send a test message to').fill('you@example.org');
      await mail.getByRole('button', { name: 'Send test e-mail' }).click();
      await settle(page, 500);
      await page.locator('#not-h').evaluate((e) => { e.scrollIntoView({ block: 'start' }); e.closest('section')?.scrollIntoView({ block: 'start' }); });
      await settle(page, 400);
      await page.screenshot({ path: `${OUT}/settings-email-${scheme}.png` });
    });
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
      // The shell sizes the popup window to its content; the browser build fits the viewport the same way.
      await page.setViewportSize({ width: 372, height: 600 });
      await page.goto('/#/tray');
      await settle(page, 1200);
      const height = await page.locator('pd-tray-popover').evaluate((e) => Math.ceil(e.getBoundingClientRect().height));
      await page.setViewportSize({ width: 372, height: height + 12 });
      await settle(page, 300);
      await page.screenshot({ path: `${OUT}/tray-${scheme}.png` });
    });
    test(`charts ${scheme}`, async ({ page }) => {
      await page.goto('/');
      await settle(page, 1400);
      await page.locator('#rel-h').evaluate((e) => e.scrollIntoView({ block: 'start' }));
      await settle(page, 700);
      await page.screenshot({ path: `${OUT}/dashboard-charts-${scheme}.png` });
    });
    test(`row menu ${scheme}`, async ({ page }) => {
      await page.goto('/#/watches');
      await settle(page, 1200);
      await page.locator('tbody tr', { hasText: 'Orbit server' }).click({ button: 'right', position: { x: 160, y: 24 } });
      await settle(page, 400);
      await page.screenshot({ path: `${OUT}/watches-menu-${scheme}.png` });
    });
    test(`feed menu ${scheme}`, async ({ page }) => {
      await page.goto('/');
      await settle(page, 1200);
      await page.locator('sp-whats-new li.item').nth(1).click({ button: 'right', position: { x: 220, y: 24 } });
      await settle(page, 400);
      await page.screenshot({ path: `${OUT}/dashboard-menu-${scheme}.png` });
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
