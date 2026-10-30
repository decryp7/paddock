import { existsSync } from 'node:fs';
import { expect, test, type Page, type TestInfo } from '@playwright/test';
import { captureScreen, expectFullyVisible, expectPageFillsHost, waitForCapturable } from '../src/index.ts';

// The capture helpers against fixture pages that reproduce the review findings of the Spotter and Pitcrew moves.

const open = (page: Page, body: string, css = '') =>
  page.goto(`data:text/html,${encodeURIComponent(`<!doctype html><style>html,body{margin:0;height:100%} body{display:grid;grid-template-rows:minmax(0,1fr)} main{overflow-y:auto;min-height:0} ${css}</style>${body}`)}`);

let shots = 0;
const out = (info: TestInfo) => info.outputPath(`shot-${++shots}.png`);
const rows = (n: number) => Array.from({ length: n }, (_, i) => `<p style="height:40px;margin:0">Row ${i}</p>`).join('');
const MAIN = (inner: string) => `<main data-pd-region="main">${inner}</main>`;

test.use({ viewport: { width: 400, height: 300 } });

test('a settled page is captured and the file is written', async ({ page }, info) => {
  await open(page, MAIN('<h1>Done</h1><p>Short page</p>'));
  const path = out(info);
  await captureScreen(page, path);
  expect(existsSync(path)).toBe(true);
});

test('refuses a page that is still loading (skeleton, aria-busy)', async ({ page }) => {
  for (const busy of ['<pd-skeleton style="display:block;height:20px">x</pd-skeleton>', '<div class="skeleton" style="height:20px">x</div>', '<div aria-busy="true">x</div>']) {
    await open(page, MAIN(busy));
    await expect(waitForCapturable(page, { timeout: 600 })).rejects.toThrow(/still loading/);
  }
});

test('waits for a skeleton that goes away, and for an image to decode', async ({ page }) => {
  await open(page, MAIN('<div class="skeleton" id="s" style="height:20px">x</div><img id="i" alt="a" width="20" height="20">'));
  await page.evaluate(() => {
    setTimeout(() => document.getElementById('s')?.remove(), 300);
    const c = document.createElement('canvas');
    c.width = c.height = 4;
    setTimeout(() => ((document.getElementById('i') as HTMLImageElement).src = c.toDataURL()), 500);
  });
  await waitForCapturable(page);
  expect(await page.evaluate(() => (document.getElementById('i') as HTMLImageElement).naturalWidth)).toBeGreaterThan(0);
});

test('refuses an image that never decodes', async ({ page }) => {
  await open(page, MAIN('<img alt="a" width="20" height="20" src="data:image/png;base64,AAAA">'));
  await expect(waitForCapturable(page, { timeout: 600 })).rejects.toThrow(/images not decoded/);
});

test('refuses a running animation in main, ignores one outside it', async ({ page }) => {
  const css = '@keyframes spin{to{transform:rotate(1turn)}} .spin{animation:spin 1s linear infinite;display:block;width:10px;height:10px;background:red}';
  await open(page, `<aside class="spin"></aside>${MAIN('<p>Calm</p>')}`, css);
  await waitForCapturable(page);
  await open(page, MAIN('<i class="spin"></i><p>x</p>'), css);
  await expect(waitForCapturable(page, { timeout: 600 })).rejects.toThrow(/animations/);
});

test('refuses an element cut at the bottom edge; fullPage main shows everything and restores the window', async ({ page }, info) => {
  await open(page, MAIN(rows(12)));
  // 12 rows of 40 px in 300: the row at 280..320 is cut
  await expect(waitForCapturable(page, { timeout: 600 })).rejects.toThrow(/cut off mid-element/);
  await captureScreen(page, out(info), { fullPage: 'main' });
  expect(page.viewportSize()).toEqual({ width: 400, height: 300 });
  await page.setViewportSize({ width: 400, height: 320 }); // eight whole rows
  await captureScreen(page, out(info));
});

test('allow lets a known cut element through', async ({ page }) => {
  await open(page, MAIN(rows(12)));
  await waitForCapturable(page, { allow: 'p' });
});

test('refuses when the frame scrolled instead of the page', async ({ page }) => {
  await open(page, `<div style="height:2000px">${MAIN('<p>x</p>')}</div>`, 'body{display:block;overflow:auto}');
  await page.evaluate(() => window.scrollTo(0, 50));
  await expect(waitForCapturable(page, { timeout: 600 })).rejects.toThrow(/frame was scrolled/);
});

test('refuses a cover painted over cut-off content, allows dialogs and real chrome', async ({ page }) => {
  const cover = '<div style="position:fixed;left:0;right:0;bottom:0;height:60px;background:#fff"></div>';
  await open(page, MAIN('<p>x</p>') + cover);
  await expect(waitForCapturable(page, { timeout: 600 })).rejects.toThrow(/overlay/);
  const dialog = '<div role="dialog" style="position:fixed;left:0;right:0;bottom:0;height:60px;background:#fff">d</div>';
  const bar = '<footer style="position:fixed;left:0;right:0;bottom:0;height:60px;background:#fff"><button>OK</button></footer>';
  await open(page, MAIN('<p>x</p>') + dialog);
  await waitForCapturable(page);
  await open(page, MAIN('<p>x</p>') + bar);
  await waitForCapturable(page);
});

test('expectFullyVisible passes for a free button and fails for a clipped, off-window or covered one', async ({ page }) => {
  await open(page, `${MAIN('<button id="ok">OK</button>')}<div id="box" style="position:fixed;left:300px;top:0;width:100px;height:30px;overflow:hidden"><button id="clipped" style="margin-top:20px;height:30px">Clip</button></div><button id="off" style="position:fixed;left:0;top:290px;height:40px">Off</button><button id="under" style="position:fixed;left:200px;top:100px;width:50px;height:30px">Under</button><div style="position:fixed;left:200px;top:100px;width:50px;height:30px;background:#ccc"></div>`);
  await expectFullyVisible(page.locator('#ok'));
  await expect(expectFullyVisible(page.locator('#clipped'), 'Clip', 500)).rejects.toThrow(/clipped/);
  await expect(expectFullyVisible(page.locator('#off'), '', 500)).rejects.toThrow(/outside the window/);
  await expect(expectFullyVisible(page.locator('#under'), '', 500)).rejects.toThrow(/covered/);
});

test('expectPageFillsHost catches a page and a virtual viewport that lost their height', async ({ page }) => {
  const fills = '.page{display:flex;flex-direction:column;height:100%} cdk-virtual-scroll-viewport{display:block;flex:1;min-height:0}';
  await open(page, MAIN('<div class="page" data-page="p"><cdk-virtual-scroll-viewport id="v"></cdk-virtual-scroll-viewport></div>'), fills);
  await expectPageFillsHost(page);
  // The bug: the page has no definite height, so the viewport's height:100% resolves to 0 (auto).
  const broken = '.page{display:block} cdk-virtual-scroll-viewport{display:block;height:100%}';
  await open(page, MAIN('<div class="page" data-page="p"><cdk-virtual-scroll-viewport id="v"></cdk-virtual-scroll-viewport></div>'), broken);
  await expect(expectPageFillsHost(page, { timeout: 500 })).rejects.toThrow();
});

const nm = (e: unknown) => String((e as Error).message);

test('rows cut by an inner scroller’s edge (a virtual list) are not a cut of the shot', async ({ page }) => {
  const rows = Array.from({ length: 30 }, (_, i) => `<div role="row" style="height:48px">Row ${i}</div>`).join('');
  await open(page, MAIN(`<div style="height:100%;overflow:auto">${rows}</div>`));
  await waitForCapturable(page, { timeout: 1500 });
});

test('an opaque panel inside main (split pane) is not a cover; covers in any colour syntax, pseudo-elements and sticky bars are', async ({ page }) => {
  await open(page, MAIN('<div style="position:relative;height:100%"><section style="position:absolute;left:0;right:0;bottom:0;height:120px;background:#eee">Details</section></div>'));
  await waitForCapturable(page, { timeout: 1500 });
  const covers: Record<string, string> = {
    pseudo: `${MAIN('<p>x</p>')}<style>body::after{content:'';position:fixed;left:0;right:0;bottom:0;height:80px;background:#fff}</style>`,
    alpha85: `${MAIN('<p>x</p>')}<div style="position:fixed;left:0;right:0;bottom:0;height:80px;background:rgba(255,255,255,.85)"></div>`,
    sticky: MAIN('<p style="height:400px">x</p><div style="position:sticky;bottom:0;height:80px;background:#fff"></div>'),
    roleStatus: `${MAIN('<p>x</p>')}<div role="status" style="position:fixed;left:0;right:0;bottom:0;height:80px;background:#fff"></div>`,
    hiddenButton: `${MAIN('<p>x</p>')}<div style="position:fixed;left:0;right:0;bottom:0;height:80px;background:#fff"><button style="opacity:0">x</button></div>`,
    colorMix: `${MAIN('<p>x</p>')}<div style="position:fixed;left:0;right:0;bottom:0;height:80px;background:color-mix(in srgb, white 100%, black)"></div>`,
    oklch: `${MAIN('<p>x</p>')}<div style="position:fixed;left:0;right:0;bottom:0;height:80px;background:oklch(1 0 0)"></div>`,
    outsideAbsolute: `${MAIN('<p>x</p>')}<div style="position:absolute;left:0;right:0;bottom:0;height:80px;background:#fff"></div>`,
  };
  for (const [name, body] of Object.entries(covers)) {
    await open(page, body);
    await expect(waitForCapturable(page, { timeout: 800 }), name).rejects.toThrow(/overlay|cover/);
  }
});

test('allow never excuses a cover', async ({ page }) => {
  await open(page, `${MAIN('<p>x</p>')}<div class="c" style="position:fixed;left:0;right:0;bottom:0;height:80px;background:#fff"></div>`);
  await expect(waitForCapturable(page, { timeout: 800, allow: '.c' })).rejects.toThrow(/cover/);
});

test('a lazy image below the fold and an SVG without intrinsic size do not hold the shot up', async ({ page }) => {
  const svg = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 1 1'/%3E";
  await open(page, MAIN(`<img alt="s" style="width:20px;height:20px" src="${svg}"><p style="height:280px;margin:0">top</p><div style="height:2000px"></div><img loading="lazy" width="20" height="20" alt="x" src="${svg}">`));
  await waitForCapturable(page, { timeout: 1500, allow: 'p' });
});

test('a spinner that is the subject can be named in animated; otherwise it holds the shot up', async ({ page }) => {
  const css = '@keyframes s{to{transform:rotate(1turn)}} .sp{animation:s 1s linear infinite;width:10px;height:10px;display:block}';
  await open(page, MAIN('<p>Still working</p><i class="sp"></i>'), css);
  await expect(waitForCapturable(page, { timeout: 600 })).rejects.toThrow(/animations/);
  await waitForCapturable(page, { animated: '.sp' });
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await open(page, MAIN('<p>Still working</p><i class="sp"></i>'), `${css} @media (prefers-reduced-motion: reduce){.sp{animation:none}}`);
  await waitForCapturable(page);
});

test('fullPage main is capped at FRAME_MAX_HEIGHT and then keeps the cut-off check', async ({ page }, info) => {
  await open(page, MAIN(Array.from({ length: 200 }, (_, i) => `<p style="height:40px;margin:0">Row ${i}</p>`).join('')));
  await captureScreen(page, out(info), { fullPage: 'main' }).then(
    () => expect(page.viewportSize()?.height).toBe(300),
    (e) => expect(nm(e)).toMatch(/cut off/),
  );
  expect(page.viewportSize()).toEqual({ width: 400, height: 300 });
});

test('expectFullyVisible waits for a dialog that is still sliding in', async ({ page }) => {
  await open(page, '<div id="d" style="position:fixed;left:50px;top:400px;transition:top .5s"><button id="ok">OK</button></div>');
  await page.evaluate(() => requestAnimationFrame(() => ((document.getElementById('d') as HTMLElement).style.top = '100px')));
  await expectFullyVisible(page.locator('#ok'));
});

test('expectFullyVisible sees a button that is covered at its bottom half only', async ({ page }) => {
  await open(page, '<button id="ok" style="position:fixed;left:10px;top:100px;height:40px;width:80px">OK</button><div style="position:fixed;left:0;top:125px;width:200px;height:40px;background:#000"></div>');
  await expect(expectFullyVisible(page.locator('#ok'), '', 600)).rejects.toThrow(/covered/);
});

test('expectPageFillsHost looks at the real page, skips unrendered viewports and has fill:false for short pages', async ({ page }) => {
  const host = 'pd-page-host{display:flex;flex-direction:column;height:100%;min-height:0} pd-part-outlet{display:flex;flex-direction:column;flex:1 1 auto;min-height:0}';
  const wrap = (inner: string) => MAIN(`<pd-page-host><pd-part-outlet class="pd-page">${inner}</pd-part-outlet></pd-page-host>`);
  // A page without a height of its own under the guarded host: the outlet fills, the page does not, its viewport collapses.
  await open(page, wrap('<x-page style="display:block"><cdk-virtual-scroll-viewport style="display:block;height:100%"></cdk-virtual-scroll-viewport></x-page>'), host);
  await expect(expectPageFillsHost(page, { timeout: 500 })).rejects.toThrow(/does not fill|collapsed/);
  await open(page, wrap('<x-page style="display:block;height:100%"><cdk-virtual-scroll-viewport style="display:block;height:100%"></cdk-virtual-scroll-viewport></x-page>'), host);
  await expectPageFillsHost(page);
  // A viewport in an inactive tab has no boxes: nothing to lose.
  await open(page, wrap('<x-page style="display:block"><div style="display:none"><cdk-virtual-scroll-viewport></cdk-virtual-scroll-viewport></div><p>short</p></x-page>'), host);
  await expectPageFillsHost(page, { fill: false });
  await expect(expectPageFillsHost(page, { timeout: 500 })).rejects.toThrow(/does not fill/);
});

test('expectFullyVisible accepts rounded buttons (4px, 6px, a 999px pill): hit tests respect border-radius', async ({ page }) => {
  for (const radius of ['4px', '6px', '999px']) {
    await open(page, `${MAIN('')}<div style="position:fixed;left:20px;top:100px"><button id="ok" style="height:32px;width:96px;border:0;background:#06c;border-radius:${radius}">OK</button></div>`);
    await expectFullyVisible(page.locator('#ok'), radius, 800);
  }
});

test('expectFullyVisible still sees a rounded button covered on one side', async ({ page }) => {
  await open(page, '<button id="ok" style="position:fixed;left:10px;top:100px;height:40px;width:80px;border-radius:999px">OK</button><div style="position:fixed;left:0;top:100px;width:200px;height:20px;background:#000"></div>');
  await expect(expectFullyVisible(page.locator('#ok'), '', 500)).rejects.toThrow(/covered/);
});

test('a broken SVG image does not count as decoded; an overflow-hidden card cut at the edge is still a cut', async ({ page }) => {
  await open(page, MAIN('<img id="b" alt="b" style="width:20px;height:20px" src="data:image/svg+xml,%3Csvg xmlns=%27http://www.w3.org/2000/svg%27%3E%3Cbroken">'));
  await expect(waitForCapturable(page, { timeout: 800 })).rejects.toThrow(/images not decoded/);
  const rows = Array.from({ length: 12 }, (_, i) => `<p style="height:40px;margin:0">Row ${i}</p>`).join('');
  await open(page, MAIN(`<div style="height:100%;overflow:hidden">${rows}</div>`));
  await expect(waitForCapturable(page, { timeout: 800 })).rejects.toThrow(/cut off/);
});
