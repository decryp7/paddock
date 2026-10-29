import { coveredInteractive, expect, expectNothingCovers, test, type Page } from '@decryp7/paddock-e2e';

// ER-21: nothing sits on top of an interactive element. The check's own fixtures first (an inert cover, a covered button, a
// sticky header over scrolled rows, an allow-listed overlap), then every gallery section (every paddock-ui component) in both themes.

const page0 = (body: string) => `<style>body { margin: 0; background: #fff; } .stage { position: relative; width: 400px; height: 200px; }
  .cover { position: absolute; left: 0; top: 0; width: 120px; height: 60px; background: rgba(0,0,0,.3); }</style><div class="stage" id="s">${body}</div>`;

test('a covered button is caught, with the cover named', async ({ page }) => {
  await page.setContent(page0('<button style="position:absolute;left:10px;top:10px;width:80px;height:30px">Save</button><div class="cover tip"></div>'));
  const found = await coveredInteractive(page.locator('#s'));
  expect(found).toHaveLength(1);
  expect(found[0]).toMatchObject({ name: 'Save', coveredBy: 'div.cover.tip', at: 'rest' });
  await expect(expectNothingCovers(page.locator('#s'))).rejects.toThrow(/1 interactive element is covered/);
});

test('a cover with pointer-events none is caught, and the probe leaves no style or state behind', async ({ page }) => {
  await page.setContent(page0('<button style="position:absolute;left:10px;top:10px;width:80px;height:30px">Save</button><div class="cover" style="pointer-events:none"></div>'));
  expect(await coveredInteractive(page.locator('#s'))).toHaveLength(1);
  expect(await page.locator('style[data-pd-hit-test]').count()).toBe(0);
  expect(await page.locator('button').evaluate((b) => getComputedStyle(b).pointerEvents)).toBe('auto');
});

test('the injected style is removed even when the probe throws', async ({ page }) => {
  await page.setContent(page0('<button style="position:absolute;left:10px;top:10px;width:80px;height:30px">Save</button>'));
  await expect(coveredInteractive(page.locator('#s'), { extra: ':::bad' })).rejects.toThrow();
  expect(await page.locator('style[data-pd-hit-test]').count()).toBe(0);
});

test('one clear point is enough: a button covered in one corner only passes', async ({ page }) => {
  await page.setContent(page0('<button style="position:absolute;left:10px;top:10px;width:80px;height:30px">Save</button><div style="position:absolute;left:0;top:0;width:20px;height:20px;background:#00f"></div>'));
  await expectNothingCovers(page.locator('#s'));
});

test('an allow-listed overlap passes, others still fail', async ({ page }) => {
  await page.setContent(page0('<button class="row-action" style="position:absolute;left:10px;top:10px;width:80px;height:30px">Save</button><div class="cover hover-card"></div>'));
  await expectNothingCovers(page.locator('#s'), { allow: [['.hover-card', '.row-action']] });
  await expect(expectNothingCovers(page.locator('#s'), { allow: [['.other', '.row-action']] })).rejects.toThrow();
});

test('a candidate clipped away by its scroller is skipped, and an extra selector adds candidates', async ({ page }) => {
  await page.setContent(
    page0(`<div id="box" style="height:60px;overflow:auto;width:300px">${Array.from({ length: 10 }, (_, i) => `<div style="height:30px"><button>Row ${i}</button></div>`).join('')}</div>
    <span class="custom" style="position:absolute;left:5px;top:105px;width:40px;height:20px">x</span><div class="cover" style="top:100px;height:40px;width:100px"></div>`),
  );
  expect(await coveredInteractive(page.locator('#s'))).toEqual([]);
  expect((await coveredInteractive(page.locator('#s'), { extra: '.custom' })).map((f) => f.selector)).toEqual(['span.custom']);
});

const sticky = (cover: string) => `<style>body { margin: 0; } .scroll { height: 200px; overflow: auto; width: 400px; } table { border-collapse: collapse; width: 100%; }
  th { position: sticky; top: 0; z-index: 2; height: 30px; background: #eee; } td { height: 28px; }</style>
  <div id="g" class="scroll"><table><thead><tr><th>Name</th><th>Size</th></tr></thead>
  <tbody>${Array.from({ length: 40 }, (_, i) => `<tr><td><button>Open ${i}</button></td><td>${i}</td></tr>`).join('')}</tbody></table>${cover}</div>`;

test('a sticky header over scrolled rows is not a cover', async ({ page }) => {
  await page.setContent(sticky(''));
  await expectNothingCovers(page.locator('#g'));
  expect(await page.locator('#g').evaluate((e) => e.scrollTop)).toBe(0); // the scroll positions are restored
});

test('a sticky header over rows that were never scrolled under it is still caught at rest', async ({ page }) => {
  await page.setContent(`<style>.scroll { height: 100px; overflow: auto; } .h { position: sticky; top: 0; height: 50px; background: #eee; margin-bottom: -50px; z-index: 2 }</style>
    <div id="g" class="scroll"><div class="h">Head</div><button style="height:30px">Under the header</button></div>`);
  const found = await coveredInteractive(page.locator('#g'), { scroll: false });
  expect(found.map((f) => f.coveredBy)).toEqual(['div.h']);
});

test('cells that paint over the sticky header do not hide the rows from the check, but a row action under a cell is caught', async ({ page }) => {
  await page.setContent(sticky('<div style="position:absolute;left:0;top:35px;width:150px;height:20px;background:#f00"></div>'));
  const found = await coveredInteractive(page.locator('#g'), { scroll: false });
  expect(found.length).toBeGreaterThan(0);
});

// Every gallery section is one component's showcase.
async function open(page: Page, theme: string): Promise<void> {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto(`/?theme=${theme}&accent=debrief`);
  await page.locator('[data-testid="card"] pd-card').first().waitFor();
}

for (const theme of ['dark', 'light'] as const) {
  test(`gallery: nothing covers an interactive element in any component (${theme})`, async ({ page }) => {
    await open(page, theme);
    const sections = await page.locator('section.gal-section').evaluateAll((all) => all.map((s) => s.getAttribute('data-testid')!));
    expect(sections.length).toBeGreaterThan(10);
    const all: string[] = [];
    for (const id of sections) {
      const found = await coveredInteractive(page.locator(`section[data-testid="${id}"]`));
      all.push(...found.map((f) => `${id}: ${f.selector} is covered by ${f.coveredBy} (${f.at})`));
    }
    expect(all, 'covered interactive elements per component').toEqual([]);
  });
}
