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

test('a cell that paints over the rows is caught', async ({ page }) => {
  await page.setContent(sticky('<div style="position:absolute;left:0;top:35px;width:150px;height:20px;background:#f00"></div>'));
  const found = await coveredInteractive(page.locator('#g'), { scroll: false });
  expect(found.length).toBeGreaterThan(0);
});

// --- must-fail fixtures from the independent review: a partial cover, a cover by a look-alike, the sticky ladder, the inert model.
const BTN = '<button id="a" style="position:absolute;left:10px;top:10px;width:80px;height:30px">Save</button>';
const names = (found: readonly { name: string }[]) => found.map((f) => f.name);

test('a tooltip over three corners and the centre of a button is caught', async ({ page }) => {
  await page.setContent(`<style>body{margin:0}</style>${BTN}<div class="tip" style="position:absolute;left:0;top:0;width:100px;height:36px;background:#333;pointer-events:none"></div>`);
  expect(names(await coveredInteractive(page.locator('body')))).toContain('Save');
});

test('a 2 px tick through the middle of a chip is caught (the centre is covered)', async ({ page }) => {
  await page.setContent(`<style>body{margin:0}</style>${BTN}<div class="tick" style="position:absolute;left:49px;top:0;width:2px;height:100px;background:#f00"></div>`);
  const found = await coveredInteractive(page.locator('body'));
  expect(found).toMatchObject([{ name: 'Save', coveredBy: 'div.tick' }]);
});

test('a tooltip over the left 75 % of a button is caught (a point is covered although the centre is not)', async ({ page }) => {
  await page.setContent(`<style>body{margin:0}</style>${BTN}<div class="tip" style="position:absolute;left:0;top:0;width:60px;height:60px;background:#333"></div>`);
  expect(names(await coveredInteractive(page.locator('body')))).toContain('Save');
});

test('a corner covered by an allow-listed overlap passes, an unlisted partial cover does not', async ({ page }) => {
  await page.setContent(`<style>body{margin:0}</style>${BTN}<div class="hover-card" style="position:absolute;left:0;top:0;width:60px;height:60px;background:#333"></div>`);
  await expectNothingCovers(page.locator('body'), { allow: [['.hover-card', '#a']] });
});

test('Material button B painted over button A is caught: its ripple and focus layers are allowed over B only', async ({ page }) => {
  await page.setContent(`<style>body{margin:0}</style>${BTN}
    <button id="b" style="position:absolute;left:0;top:0;width:120px;height:60px;z-index:2">Other<span class="mat-mdc-button-persistent-ripple" style="position:absolute;inset:0;pointer-events:none"></span><span class="mat-focus-indicator" style="position:absolute;inset:0;pointer-events:none"></span></button>`);
  const found = await coveredInteractive(page.locator('body'));
  expect(names(found)).toContain('Save');
  expect(names(found)).not.toContain('Other'); // B's own ripple over B is Material's painting
});

test('a button covers only itself: a ripple inside its own control passes', async ({ page }) => {
  await page.setContent(`<style>body{margin:0}</style><button style="position:absolute;left:10px;top:10px;width:80px;height:30px">Own<span class="mat-ripple" style="position:absolute;inset:0;pointer-events:none"></span></button>`);
  await expectNothingCovers(page.locator('body'));
});

test('a pseudo-element cover with pointer-events none is caught', async ({ page }) => {
  await page.setContent(`<style>body{margin:0} .host{position:absolute;left:0;top:0;width:0;height:0} .host::after{content:'';position:absolute;left:0;top:0;width:120px;height:60px;background:rgba(0,0,0,.5);pointer-events:none}</style>${BTN}<div class="host"></div>`);
  expect(names(await coveredInteractive(page.locator('body')))).toContain('Save');
});

test('an empty, transparent, inert fixed layer (an overlay container) covers nothing', async ({ page }) => {
  await page.setContent(`<style>body{margin:0}</style>${BTN}<div class="layer" style="position:fixed;inset:0;pointer-events:none;z-index:1000"></div>`);
  await expectNothingCovers(page.locator('body'));
});

test('an inert layer that paints something (a tint, a border, text, a shadow) is a cover', async ({ page }) => {
  for (const paint of ['background:rgba(0,0,0,.05)', 'border:1px solid #888', 'box-shadow:0 0 4px #000', 'filter:blur(1px)']) {
    await page.setContent(`<style>body{margin:0}</style>${BTN}<div class="paint" style="position:fixed;inset:0;pointer-events:none;${paint}"></div>`);
    expect(names(await coveredInteractive(page.locator('body'))), paint).toContain('Save');
  }
  await page.setContent(`<style>body{margin:0}</style>${BTN}<div class="paint" style="position:fixed;inset:0;pointer-events:none">label</div>`);
  expect(names(await coveredInteractive(page.locator('body')))).toContain('Save');
});

test('a red, click-blocking div over a button is caught and named', async ({ page }) => {
  await page.setContent(`<style>body{margin:0}</style>${BTN}<div class="bug" style="position:fixed;left:0;top:0;width:120px;height:60px;background:red;z-index:5"></div>`);
  expect(await coveredInteractive(page.locator('body'))).toMatchObject([{ name: 'Save', coveredBy: 'div.bug' }]);
  // An invisible but click-blocking layer is a cover too: it took the click.
  await page.setContent(`<style>body{margin:0}</style>${BTN}<div class="ghost" style="position:fixed;inset:0;z-index:5"></div>`);
  expect(await coveredInteractive(page.locator('body'))).toMatchObject([{ name: 'Save', coveredBy: 'div.ghost' }]);
});

test('(C) a sticky first column over the sticky header after a vertical scroll is caught', async ({ page }) => {
  await page.setContent(`<style>body{margin:0} .scroll{height:200px;width:300px;overflow:auto}
    table{border-collapse:collapse;width:600px} th{position:sticky;top:0;z-index:1;height:30px;background:#eee}
    td{height:28px} td.first{position:sticky;left:0;z-index:3;background:#fdd}</style>
    <div id="g" class="scroll"><table><thead><tr><th><button>Sort name</button></th><th>Size</th></tr></thead>
    <tbody>${Array.from({ length: 40 }, (_, i) => `<tr><td class="first">r${i}</td><td>${i}</td></tr>`).join('')}</tbody></table></div>`);
  const found = await coveredInteractive(page.locator('#g'));
  expect(names(found)).toContain('Sort name');
  expect(found.find((f) => f.name === 'Sort name')!.at).toMatch(/scrolled/);
});

test('(D) after the page scrolls, a sticky banner over a fixed button is caught', async ({ page }) => {
  await page.setContent(`<style>body{margin:0}</style><div style="height:20px"></div><div class="banner" style="position:sticky;top:0;height:60px;background:#fc0;z-index:5"></div>
    <button style="position:fixed;left:10px;top:10px;width:80px;height:30px;z-index:1">Fixed</button><div style="height:3000px"></div>`);
  expect(names(await coveredInteractive(page.locator('body'), { scroll: false }))).toContain('Fixed');
  expect(names(await coveredInteractive(page.locator('body')))).toContain('Fixed');
});

test('a sticky header over rows that scrolled under it is still not a cover, and each scroller is restored before the next', async ({ page }) => {
  await page.setContent(`<style>body{margin:0}</style><div id="g" style="height:600px"><div id="one" style="height:100px;overflow:auto"><div style="height:30px;position:sticky;top:0;background:#eee;z-index:2">H1</div><div style="height:400px"><button style="margin-top:60px">One</button></div></div>
    <div id="two" style="height:100px;overflow:auto;margin-top:20px"><div style="height:30px;position:sticky;top:0;background:#eee;z-index:2">H2</div><div style="height:400px"><button style="margin-top:60px">Two</button></div></div></div>`);
  await expectNothingCovers(page.locator('#g'));
  expect(await page.evaluate(() => [document.getElementById('one')!.scrollTop, document.getElementById('two')!.scrollTop, scrollY])).toEqual([0, 0, 0]);
});

test('a cover that only shows after scrolling is found by the scroll probe, not by scroll: false', async ({ page }) => {
  await page.setContent(`<style>body{margin:0}</style><div id="g" style="position:relative;height:100px;overflow:auto;width:300px"><div style="height:800px;position:relative">
    <button style="position:absolute;left:10px;top:500px;width:80px;height:30px">Late</button><div class="late-cover" style="position:absolute;left:0;top:490px;width:120px;height:60px;background:#333"></div></div></div>`);
  expect(await coveredInteractive(page.locator('#g'), { scroll: false })).toEqual([]);
  const found = await coveredInteractive(page.locator('#g'));
  expect(found).toMatchObject([{ name: 'Late', coveredBy: 'div.late-cover' }]);
  expect(found[0]!.at).toMatch(/scrolled to/);
});

test('an absolute button that escapes a non-positioned overflow:hidden parent is still probed', async ({ page }) => {
  await page.setContent(`<style>body{margin:0}</style><div style="position:relative;height:300px"><div style="overflow:hidden;height:20px;width:300px">
    <button style="position:absolute;left:10px;top:100px;width:80px;height:30px">Escaped</button></div>
    <div style="position:absolute;left:0;top:90px;width:120px;height:60px;background:#333"></div></div>`);
  expect(names(await coveredInteractive(page.locator('body')))).toContain('Escaped');
});

test('a label: the label element over its input passes (a custom checkbox), a popover inside the label does not', async ({ page }) => {
  await page.setContent(`<style>body{margin:0}</style><label id="l" style="position:relative;display:block;width:300px;height:30px"><input id="n" type="checkbox" style="position:absolute;left:0;top:0;width:20px;height:20px"></label>`);
  await expectNothingCovers(page.locator('#l'));
  await page.setContent(`<style>body{margin:0}</style><label style="position:relative;display:block;width:300px">Name <input id="n" style="width:150px">
    <span class="pop" style="position:absolute;left:0;top:-5px;width:300px;height:40px;background:#333">popover</span></label>`);
  expect(await coveredInteractive(page.locator('label'))).toMatchObject([{ selector: 'input#n', coveredBy: 'span.pop' }]);
});

test('a rounded corner that lands on the ancestor still reaches the button', async ({ page }) => {
  await page.setContent(`<style>body{margin:0}</style><div id="p" style="position:absolute;left:0;top:0;width:200px;height:80px;background:#ddd"><button style="position:absolute;left:20px;top:20px;width:100px;height:40px;border-radius:20px">Pill</button></div>`);
  await expectNothingCovers(page.locator('#p'));
});

test('five identical covered buttons are five findings', async ({ page }) => {
  await page.setContent(`<style>body{margin:0}</style>${Array.from({ length: 5 }, (_, i) => `<button class="ann" style="position:absolute;left:${i * 50}px;top:10px;width:40px;height:20px">A${i}</button>`).join('')}
    <div class="blanket" style="position:absolute;left:0;top:0;width:300px;height:60px;background:#333"></div>`);
  expect(await coveredInteractive(page.locator('body'))).toHaveLength(5);
});

test('the probe leaves the page scroll where it was', async ({ page }) => {
  await page.setContent(`<style>body{margin:0}</style><div style="height:2000px"></div><div id="far"><button>Far</button></div><div style="height:2000px"></div>`);
  expect(await page.evaluate(() => scrollY)).toBe(0);
  await expectNothingCovers(page.locator('#far'));
  expect(await page.evaluate(() => scrollY)).toBe(0);
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
