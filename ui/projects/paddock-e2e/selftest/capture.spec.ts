import { mkdtempSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { expect, test, type Page } from '@playwright/test';
import { captureScreen, expectFullyVisible, expectPageFillsHost, waitForCapturable } from '../src/index.ts';

// The capture helpers against fixture pages that reproduce the review findings of the Spotter and Pitcrew moves.

const open = (page: Page, body: string, css = '') =>
  page.goto(`data:text/html,${encodeURIComponent(`<!doctype html><style>html,body{margin:0;height:100%} body{display:grid;grid-template-rows:minmax(0,1fr)} main{overflow-y:auto;min-height:0} ${css}</style>${body}`)}`);

const out = () => join(mkdtempSync(join(tmpdir(), 'pd-capture-')), 'shot.png');
const rows = (n: number) => Array.from({ length: n }, (_, i) => `<p style="height:40px;margin:0">Row ${i}</p>`).join('');
const MAIN = (inner: string) => `<main data-pd-region="main">${inner}</main>`;

test.use({ viewport: { width: 400, height: 300 } });

test('a settled page is captured and the file is written', async ({ page }) => {
  await open(page, MAIN('<h1>Done</h1><p>Short page</p>'));
  const path = out();
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

test('refuses an element cut at the bottom edge; fullPage main shows everything and restores the window', async ({ page }) => {
  await open(page, MAIN(rows(12)));
  // 12 rows of 40 px in 300: the row at 280..320 is cut
  await expect(waitForCapturable(page, { timeout: 600 })).rejects.toThrow(/cut off mid-element/);
  await captureScreen(page, out(), { fullPage: 'main' });
  expect(page.viewportSize()).toEqual({ width: 400, height: 300 });
  await page.setViewportSize({ width: 400, height: 320 }); // eight whole rows
  await captureScreen(page, out());
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
  await expect(expectFullyVisible(page.locator('#clipped'), 'Clip')).rejects.toThrow(/clipped/);
  await expect(expectFullyVisible(page.locator('#off'))).rejects.toThrow(/outside the window/);
  await expect(expectFullyVisible(page.locator('#under'))).rejects.toThrow(/covered/);
});

test('expectPageFillsHost catches a page and a virtual viewport that lost their height', async ({ page }) => {
  const fills = '.page{display:flex;flex-direction:column;height:100%} cdk-virtual-scroll-viewport{display:block;flex:1;min-height:0}';
  await open(page, MAIN('<div class="page" data-page="p"><cdk-virtual-scroll-viewport id="v"></cdk-virtual-scroll-viewport></div>'), fills);
  await expectPageFillsHost(page);
  // The bug: the page has no definite height, so the viewport's height:100% resolves to 0 (auto).
  const broken = '.page{display:block} cdk-virtual-scroll-viewport{display:block;height:100%}';
  await open(page, MAIN('<div class="page" data-page="p"><cdk-virtual-scroll-viewport id="v"></cdk-virtual-scroll-viewport></div>'), broken);
  await expect(expectPageFillsHost(page)).rejects.toThrow();
});
