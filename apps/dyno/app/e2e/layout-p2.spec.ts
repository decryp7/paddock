import { expect, test, type Page } from '@decryp7/paddock-e2e';

/**
 * The layout re-review's P2 findings (Dyno 1.1.1 plan WP-14 e), each as a measured condition (ER-12: conditions, never sleeps):
 *  - Extras: the content is as wide on every tab, so switching tabs does not make the page jump;
 *  - Network > Listening: every toolbar control, the expand and collapse buttons too, stays on one row at 1280 and at 1240;
 *  - Network > Connections: no column takes the whole free width at 1920 (an empty band between Remote and Protocol);
 *  - Firewall: the "Turn on administrator readings" button is as wide as its words, not the whole card, at 1280;
 *  - Alarms: the Alarm column shows a long title in full at 1280;
 *  - Processes: the subtitle stays on one line at 1024 and 1280.
 */
async function open(page: Page, size: { width: number; height: number }, hash: string): Promise<void> {
  await page.setViewportSize(size);
  await page.goto(`/?latency=0&rows=many#${hash}`);
  await expect(page.getByRole('heading', { level: 1 }).first()).toBeVisible({ timeout: 45_000 });
  await page.waitForFunction(() => !document.querySelector('[aria-busy="true"]'), undefined, { timeout: 30_000 });
}

test('extras: the content is as wide on every tab at 1920', async ({ page }) => {
  const widths: Record<string, number> = {};
  for (const tab of ['changes', 'inventory', 'boot', 'apps']) {
    await open(page, { width: 1920, height: 1080 }, `/extras?tab=${tab}`);
    widths[tab] = await page.evaluate(() => Math.round((document.querySelector('#pd-main .page') as HTMLElement).getBoundingClientRect().width));
  }
  const all = Object.values(widths);
  expect(Math.max(...all) - Math.min(...all), `the page width per tab: ${JSON.stringify(widths)}`).toBeLessThanOrEqual(1);
});

test('network, listening: the toolbar controls are one line each at 1280', async ({ page }) => {
  await open(page, { width: 1280, height: 800 }, '/network');
  await page.getByRole('radio', { name: 'Listening' }).click();
  const lines = await page.evaluate(() =>
    [...document.querySelectorAll<HTMLElement>('#pd-main .mat-mdc-button-base, #pd-main label, #pd-main [role=radio]')]
      .filter((e) => e.getBoundingClientRect().height > 0 && e.getBoundingClientRect().top < 300)
      .map((e) => ({ text: (e.textContent ?? '').trim(), height: Math.round(e.getBoundingClientRect().height), line: parseFloat(getComputedStyle(e).lineHeight) || 20 })),
  );
  expect(lines.length).toBeGreaterThan(3);
  for (const l of lines) expect(l.height, `"${l.text}" wraps`).toBeLessThan(l.line * 2 + 4);
});

// 1280 fits with no room to spare (the search field sits at its minimum), so 1240 stands for a wider font or a wider scrollbar.
for (const width of [1280, 1240]) {
test(`network, listening: every toolbar control, the expand and collapse buttons included, shares one row at ${width}`, async ({ page }) => {
  await open(page, { width, height: 800 }, '/network');
  await page.getByRole('radio', { name: 'Listening' }).click();
  await expect(page.getByRole('treegrid', { name: 'Listening ports' })).toBeVisible();
  await page.waitForFunction(() => !document.querySelector('[aria-busy="true"]'));
  const tops = await page.evaluate(() => {
    const names = [/^Connections$/, /^Listening$/, /^Show loopback$/, /^Exposed only$/, /^All$/, /^TCP$/, /^UDP$/, /^None$/, /^Program$/];
    const own = [...document.querySelectorAll<HTMLElement>('#pd-main label, #pd-main [role=radio], #pd-main button')].filter((e) => e.getBoundingClientRect().height > 0);
    const found = names.map((re) => own.find((e) => re.test((e.textContent ?? '').trim())));
    const actions = [...document.querySelectorAll<HTMLElement>('[data-testid=tree-expand-all], [data-testid=tree-collapse-all]')];
    return [...found, ...actions].map((e) => (e ? Math.round(e.getBoundingClientRect().top + e.getBoundingClientRect().height / 2) : -1));
  });
  expect(tops.length).toBe(11);
  expect(tops.every((t) => t > 0), `a toolbar control was not found: ${JSON.stringify(tops)}`).toBe(true);
  expect(Math.max(...tops) - Math.min(...tops), `the centres of the toolbar controls: ${JSON.stringify(tops)}`).toBeLessThanOrEqual(2);
});
}

test('network, connections: the Service column does not take the free width at 1920', async ({ page }) => {
  await open(page, { width: 1920, height: 1080 }, '/network');
  const w = await page.evaluate(() => {
    const cell = (label: string) => [...document.querySelectorAll<HTMLElement>('#pd-main [role=columnheader]')].find((h) => (h.textContent ?? '').trim() === label);
    return { service: Math.round(cell('Service')!.getBoundingClientRect().width), remote: Math.round(cell('Remote')!.getBoundingClientRect().width) };
  });
  expect(w.service, `column widths: ${JSON.stringify(w)}`).toBeLessThanOrEqual(w.remote * 1.1);
});

test('firewall: the administrator-readings button is not the card width at 1280', async ({ page }) => {
  await open(page, { width: 1280, height: 800 }, '/firewall');
  const b = page.getByRole('button', { name: 'Turn on administrator readings' });
  await expect(b).toBeVisible();
  const box = (await b.boundingBox())!;
  expect(box.width, 'the button is as wide as its words').toBeLessThanOrEqual(320);
});

test('alarms: a long title shows in full at 1280', async ({ page }) => {
  await open(page, { width: 1280, height: 800 }, '/alarms');
  const cut = await page.evaluate(() => {
    const cells = [...document.querySelectorAll<HTMLElement>('#pd-main [role=gridcell], #pd-main [role=cell]')].filter((c) => (c.textContent ?? '').includes('GPU load is over'));
    return cells.map((c) => {
      const t = [c, ...c.querySelectorAll<HTMLElement>('*')].reduce((m, e) => Math.max(m, e.scrollWidth - e.clientWidth), 0);
      return t;
    });
  });
  expect(cut.length).toBeGreaterThan(0);
  for (const c of cut) expect(c, 'the title is cut').toBeLessThanOrEqual(0);
});

for (const width of [1024, 1280]) {
  test(`processes: the subtitle is one line at ${width}`, async ({ page }) => {
    await open(page, { width, height: 800 }, '/processes');
    const h = await page.evaluate(() => {
      const p = document.querySelector('#pd-main .titles p') as HTMLElement;
      return { height: p.getBoundingClientRect().height, line: parseFloat(getComputedStyle(p).lineHeight) };
    });
    expect(h.height, `the subtitle wraps: ${JSON.stringify(h)}`).toBeLessThan(h.line * 1.5);
  });
}

/** Decision 1 of the plan: when the table is narrow, Disk goes first and Account stays (Disk is also in the details pane). */
test('processes: a narrow table drops Disk before Account', async ({ page }) => {
  await open(page, { width: 900, height: 800 }, '/processes');
  const headers = await page.evaluate(() => [...document.querySelectorAll('#pd-main [role=columnheader]')].map((h) => (h.textContent ?? '').trim()));
  expect(headers, `the visible columns: ${headers.join(', ')}`).toContain('Account');
  expect(headers).not.toContain('Disk');
});
