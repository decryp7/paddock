import { expect, expectHeaderOnTop, headerCovers, headerTrack, test, type Page } from '@decryp7/paddock-e2e';

// ER-21: scrolled rows and cells never paint over a grid's header (the user's report of 2026-10-28: "the cell will cover the
// header when scrolling"). Every gallery grid is scrolled, hovered and probed with elementFromPoint across its header row.

async function open(page: Page, theme: string): Promise<void> {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto(`/?theme=${theme}&accent=debrief`);
  await page.locator('section[data-testid="table"] pd-table').first().waitFor();
}

for (const theme of ['dark', 'light'] as const) {
  test(`pd-table: the virtual table's header stays on top while it scrolls and a row is hovered (${theme})`, async ({ page }) => {
    await open(page, theme);
    await expectHeaderOnTop(page.locator('[data-testid="table-virtual"] .pd-grid'));
  });

  // Deep in the 5,000 rows the viewport draws a window far from the top and moves it under the sticky header (the CDK
  // table keeps the header at the top by its rendered offset): the header must stay on top there too.
  test(`pd-table: deep in 5,000 virtual rows the header stays on top while scrolled and hovered (${theme})`, async ({ page }) => {
    await open(page, theme);
    await expectHeaderOnTop(page.locator('[data-testid="table-virtual"] .pd-grid'), { from: 2500 * 32, steps: 8 });
  });

  // Frame by frame: 300 px a frame for 60 frames, a jump of 2.5 viewports, then the end. The header must not move and must be
  // on top in every frame (review P1-1: a header inside the virtual list's moving window left the top on fast scrolls).
  test(`pd-table: the header stays put and on top in every frame of a fast scroll, a jump and End (${theme})`, async ({ page }) => {
    await open(page, theme);
    const frames = await headerTrack(page.locator('[data-testid="table-virtual"] .pd-grid'), { perFrame: 300, frames: 60 });
    expect(frames.length).toBe(62);
    expect(frames.at(-1)!.scrollTop).toBeGreaterThan(100_000);
    expect(frames.filter((f) => f.moved !== 0 || !f.onTop)).toEqual([]);
  });
}

// The check itself: a grid whose cells paint over its sticky header must fail it, and the same grid with the header raised must pass.
const fixture = (cell: string, head: string) => `
  <style>
    body { margin: 0; background: #fff; }
    .scroll { height: 200px; overflow: auto; width: 400px; }
    table { border-collapse: collapse; width: 100%; }
    th { position: sticky; top: 0; height: 30px; background: rgb(240, 240, 240); ${head} }
    td { height: 28px; } td span { ${cell} }
  </style>
  <div class="grid"><div class="scroll"><table><thead><tr><th>Name</th><th>Size</th></tr></thead>
  <tbody>${Array.from({ length: 40 }, (_, i) => `<tr><td><span>Row ${i}</span></td><td><span>${i * 3}</span></td></tr>`).join('')}</tbody></table></div></div>`;

test('the check fails on a sticky header without a z-index above a positioned cell', async ({ page }) => {
  await page.setContent(fixture('position: relative; display: block;', ''));
  const report = await headerCovers(page.locator('.grid'), { header: 'thead' });
  test.expect(report.scrolled).toBeGreaterThan(0);
  test.expect(report.covers.length).toBeGreaterThan(0);
});

test('the check fails on a header that lets the rows show through', async ({ page }) => {
  await page.setContent(fixture('', 'z-index: 2; background: rgba(240, 240, 240, 0.6);'));
  const report = await headerCovers(page.locator('.grid'), { header: 'thead' });
  test.expect(report.opaque).toBe(false);
});

test('the check passes once the header is above the cells', async ({ page }) => {
  await page.setContent(fixture('position: relative; display: block;', 'z-index: 2;'));
  await expectHeaderOnTop(page.locator('.grid'), { header: 'thead' });
});
