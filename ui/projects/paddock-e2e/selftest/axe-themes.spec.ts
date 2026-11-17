import { expect, test, type Page } from '@playwright/test';
import { axeInThemes, axeViolations } from '../src/index.ts';

// Pitwall run 8134 (WebKit): axe read Benchmarks' muted cells on a row half-way through the theme's 100 ms background
// transition (#a0a7b2 on #5c5d60, 2.71:1). Colours read while they change are not a measurement, so axeViolations holds the
// page still for the scan (no transitions or animations, reduced motion). These pages start motion from inside axe's own
// scan (`onEveryScan`), so the order is certain: before the change, axe read them mid-change.

/** Runs `body` (the source of a function of the scan number `n`) in the page at the start of each of axe's scans. */
async function onEveryScan(page: Page, body: string): Promise<void> {
  await page.evaluate((source) => {
    type Run = (...a: unknown[]) => Promise<unknown>;
    const w = window as unknown as { scans: number; onScan: (n: number) => void };
    w.scans = 0;
    w.onScan = new Function('n', source) as (n: number) => void;
    let real: object | undefined;
    // axe-core assigns window.axe first and its runPartial later: wrap runPartial whenever it is assigned.
    Object.defineProperty(window, 'axe', {
      configurable: true,
      get: () => real,
      set: (axe: object) => {
        let run: Run | undefined;
        const wrapped: Run = async (...args) => {
          w.onScan(++w.scans);
          return (run as Run)(...args);
        };
        Object.defineProperty(axe, 'runPartial', { configurable: true, get: () => (run ? wrapped : undefined), set: (f: Run) => (run = f) });
        real = axe;
      },
    });
  }, body);
}

const open = (page: Page, html: string) => page.goto(`data:text/html,${encodeURIComponent(`<!doctype html><html lang="en"><title>Motion</title>${html}</html>`)}`);

// Text that is fine on its end background, and fails on the grey an animation starts from.
const CELL = 'td { background: rgb(255, 255, 255); color: rgb(85, 85, 85); }';
const FLASH = '@keyframes flash { from { background: rgb(110, 110, 110); } to { background: rgb(255, 255, 255); } }';

test('a 90 ms colour animation that starts as the scan begins is not read half-way', async ({ page }) => {
  await open(page, `<style>${CELL} ${FLASH} .go td { animation: flash 90ms linear; }</style><table><tbody><tr><td>muted text</td></tr></tbody></table>`);
  await onEveryScan(page, "document.documentElement.classList.add('go');");
  expect(await axeViolations(page)).toEqual([]);
  expect(await page.evaluate(() => (window as unknown as { scans: number }).scans), 'the probe ran in the scan').toBe(1);
});

test('the same animation on a ::before is not read half-way either', async ({ page }) => {
  const style = `td::before { content: 'before '; background: rgb(255, 255, 255); color: rgb(85, 85, 85); } ${FLASH}
    .go td::before { animation: flash 90ms linear; }`;
  await open(page, `<style>${CELL} ${style}</style><table><tbody><tr><td>muted text</td></tr></tbody></table>`);
  // axe judges text against what lies behind it, and reads a pseudo-element's colours only partly, so the probe also reads
  // the ::before's background as the scan starts: its end state, not the animation's first frame.
  await onEveryScan(page, "document.documentElement.classList.add('go'); window.seen = getComputedStyle(document.querySelector('td'), '::before').backgroundColor;");
  expect(await axeViolations(page)).toEqual([]);
  expect(await page.evaluate(() => (window as unknown as { seen: string }).seen)).toBe('rgb(255, 255, 255)');
});

test('a fill-opacity transition started in the scan is at its end state while axe reads', async ({ page }) => {
  await open(page, `<style>rect { fill-opacity: 0.2; transition: fill-opacity 3s linear; } .go rect { fill-opacity: 1; }</style>
    <svg width="40" height="40" aria-hidden="true"><rect id="r" width="40" height="40" fill="rgb(0, 0, 0)" /></svg><p>text</p>`);
  await onEveryScan(page, "document.documentElement.classList.add('go'); window.seen = getComputedStyle(document.getElementById('r')).fillOpacity;");
  expect(await axeViolations(page)).toEqual([]);
  expect(await page.evaluate(() => (window as unknown as { seen: string }).seen)).toBe('1');
});

test('an overlay that fades in with fill-mode forwards (Material tooltip) is scanned open, not invisible', async ({ page }) => {
  // Material's tooltip panel starts at opacity 0 and scale 0.8 and keeps its keyframes' end with fill-mode forwards
  // (mat-mdc-tooltip-show, 150 ms); its select panel and snack bar do the same. The tooltip text here fails contrast.
  await open(
    page,
    `<style>
      @keyframes tip-show { from { opacity: 0; transform: scale(0.8); } to { opacity: 1; transform: scale(1); } }
      .tip { opacity: 0; transform: scale(0.8); background: rgb(255, 255, 255); color: rgb(190, 190, 190); }
      .tip.shown { animation: tip-show 150ms cubic-bezier(0, 0, 0.2, 1) forwards; }
    </style><p>host</p><div id="tip" class="tip" role="tooltip">tooltip text</div>`,
  );
  await page.evaluate(() => document.querySelector('.tip')?.classList.add('shown'));
  expect(await axeViolations(page)).toEqual(['color-contrast: #tip']);
});

test('axeViolations still reports a real contrast failure', async ({ page }) => {
  await open(page, `<style>td { background: rgb(255, 255, 255); color: rgb(200, 200, 200); }</style><table><tbody><tr><td>faint text</td></tr></tbody></table>`);
  expect(await axeViolations(page, { idsOnly: true })).toEqual(['color-contrast']);
});

test('axeInThemes reads each theme at its end state, not the switch into it', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'light' });
  await open(
    page,
    // Only the background moves (3 s): the text turns light at once, so mid-way it is light text on a pale background.
    `<style>td { background: rgb(255, 255, 255); color: rgb(85, 85, 85); transition: background 3s linear; }
      @media (prefers-color-scheme: dark) { td { background: rgb(17, 17, 17); color: rgb(160, 160, 160); } }</style>
    <table><tbody><tr><td>muted text</td></tr></tbody></table>`,
  );
  expect(await axeInThemes(page, { themes: ['dark', 'light'] })).toEqual([]);
  // Motion is back after the scan: the page's transitions and its reduced-motion preference are as they were.
  expect(
    await page.evaluate(() => [getComputedStyle(document.querySelector('td') as HTMLElement).transitionDuration, matchMedia('(prefers-reduced-motion: reduce)').matches]),
  ).toEqual(['3s', false]);
});
