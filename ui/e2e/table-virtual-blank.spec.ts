import { expect, test, type Locator, type Page } from '@decryp7/paddock-e2e';

// The user's report (Dyno 1.1.0 RC, Processes, 458 rows sorted by CPU): scrolled near the end, the table showed blank space
// under the last drawn row while more rows existed. The body must always be filled to its bottom edge while rows remain.

async function open(page: Page): Promise<Locator> {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/?theme=dark&accent=debrief');
  const box = page.locator('[data-testid="table-virtual"]');
  await box.locator('cdk-virtual-scroll-viewport').waitFor();
  return box;
}

/** The gap in px between the lowest drawn row and the bottom of the scrolling body, and whether the body is scrolled to its end. */
async function gap(box: Locator): Promise<{ gap: number; atEnd: boolean; drawn: number }> {
  return box.locator('cdk-virtual-scroll-viewport').evaluate((vp) => {
    const rows = [...vp.querySelectorAll<HTMLElement>('[role=row][data-index]')];
    const bottom = Math.max(...rows.map((r) => r.getBoundingClientRect().bottom));
    const body = vp.getBoundingClientRect();
    return { gap: Math.round(body.bottom - bottom), atEnd: Math.abs(vp.scrollHeight - vp.clientHeight - vp.scrollTop) < 2, drawn: rows.length };
  });
}

test('table: scrolled to the end of a long virtual table, the last row meets the body bottom', async ({ page }) => {
  const box = await open(page);
  await box.locator('cdk-virtual-scroll-viewport').evaluate((vp) => vp.scrollTo({ top: vp.scrollHeight }));
  await expect.poll(async () => (await gap(box)).atEnd).toBe(true);
  await expect.poll(async () => (await gap(box)).gap).toBeLessThanOrEqual(1);
});

test('table: the filter shrinks the list while scrolled to the end, and no blank space is left', async ({ page }) => {
  const box = await open(page);
  await box.locator('cdk-virtual-scroll-viewport').evaluate((vp) => vp.scrollTo({ top: vp.scrollHeight }));
  await expect.poll(async () => (await gap(box)).atEnd).toBe(true);
  await box.locator('.pd-filter input').fill('1');
  await expect.poll(async () => (await gap(box)).gap, { message: 'rows fill the body after the list shrank' }).toBeLessThanOrEqual(1);
});

// Root cause: the CDK viewport measures its height at start and on a window resize only, so a body that grows later (Dyno's
// frame-height-derived maxHeight, rows arriving after a skeleton) draws only the rows its old height and buffer cover.
test('table: the body grows taller than the rows drawn for its old height, and no blank space is left', async ({ page }) => {
  const box = await open(page);
  const vp = box.locator('cdk-virtual-scroll-viewport');
  await vp.evaluate((e) => ((e as HTMLElement).style.height = '1600px'));
  await expect.poll(async () => (await gap(box)).gap).toBeLessThanOrEqual(1);
  await vp.evaluate((e) => e.scrollTo({ top: e.scrollHeight }));
  await expect.poll(async () => (await gap(box)).atEnd).toBe(true);
  await expect.poll(async () => (await gap(box)).gap).toBeLessThanOrEqual(1);
});
