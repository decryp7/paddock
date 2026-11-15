import { expect, test, type Page } from '@playwright/test';
import { axeViolations } from '../src/index.ts';

// Pitwall run 8134 (WebKit): axe read Benchmarks' muted cells on a row half-way through the theme's 100 ms background
// transition (#a0a7b2 on #5c5d60, 2.71:1), although the transitions had been settled before the scan: the transition started
// while axe ran. Colours read mid-transition are not a measurement: axeViolations sees that a colour moved during the scan,
// says so, settles and scans again. These pages start motion from inside axe's own scan (`onEveryScan`), so the order is
// certain.
const table = (dark: string) => `<!doctype html><html lang="en"><title>Themes</title><style>
    td { background: rgb(255, 255, 255); color: rgb(85, 85, 85); transition: background 3s linear; }
    .dark td { ${dark} }
  </style>
  <table><tbody><tr><td>muted text</td><td>more muted text</td></tr></tbody></table></html>`;
const GOOD_DARK = table('background: rgb(17, 17, 17); color: rgb(160, 160, 160);');
const BAD_DARK = table('background: rgb(17, 17, 17); color: rgb(60, 60, 60);');

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

const scans = (page: Page) => page.evaluate(() => (window as unknown as { scans: number }).scans);
const rescans = () => test.info().annotations.filter((a) => a.type === 'axe-rescan');

test('axeViolations does not report colours read while a transition that began during the scan runs', async ({ page }) => {
  await page.goto(`data:text/html,${encodeURIComponent(GOOD_DARK)}`);
  // The text turns light at once, the background follows over 3 s: light on white until then.
  await onEveryScan(page, "if (n === 1) document.documentElement.classList.add('dark');");
  expect(await axeViolations(page)).toEqual([]);
  expect(await scans(page), 'the first scan was discarded and a second one made').toBe(2);
  expect(rescans().map((a) => a.description)).toEqual([
    expect.stringMatching(/^scan 1 of 4 counted for nothing: transition background-color started during it; .*prefers-color-scheme: dark matches/),
  ]);
});

test('axeViolations still reports a real contrast failure', async ({ page }) => {
  await page.goto(`data:text/html,${encodeURIComponent(table('').replace('rgb(85, 85, 85)', 'rgb(200, 200, 200)'))}`);
  expect(await axeViolations(page, { idsOnly: true })).toEqual(['color-contrast']);
});

test('motion in the first scan into a theme that fails is still reported by the next scan', async ({ page }) => {
  await page.goto(`data:text/html,${encodeURIComponent(BAD_DARK)}`);
  await onEveryScan(page, "if (n === 1) document.documentElement.classList.add('dark');");
  expect(await axeViolations(page, { idsOnly: true })).toEqual(['color-contrast']);
  expect(await scans(page)).toBe(2);
});

test('axeViolations throws, naming what moved, when every scan sees a colour start', async ({ page }) => {
  await page.goto(`data:text/html,${encodeURIComponent(GOOD_DARK)}`);
  await onEveryScan(page, "document.documentElement.classList.toggle('dark');");
  await expect(axeViolations(page)).rejects.toThrow(/colours kept changing during 4 scans \(transition background-color\)/);
  expect(await scans(page)).toBe(4);
  expect(rescans()).toHaveLength(4);
});

test('a ticking stroke-dashoffset (a progress ring) is not colour motion', async ({ page }) => {
  const ring = `<!doctype html><html lang="en"><title>Ring</title><style>circle { transition: stroke-dashoffset 1s linear; }</style>
    <svg width="40" height="40" aria-hidden="true"><circle id="r" cx="20" cy="20" r="15" fill="none" stroke="rgb(0, 0, 0)" stroke-dasharray="94" stroke-dashoffset="0" /></svg>
    <p>ring</p></html>`;
  await page.goto(`data:text/html,${encodeURIComponent(ring)}`);
  await onEveryScan(page, "document.getElementById('r').style.strokeDashoffset = String(10 * n);");
  expect(await axeViolations(page)).toEqual([]);
  expect(await scans(page)).toBe(1);
});

test("an empty animation (CDK's autofill detector) is not colour motion", async ({ page }) => {
  const form = `<!doctype html><html lang="en"><title>Form</title><style>
      @keyframes cdk-text-field-autofill-start { /*!*/ }
      input.watch { animation: cdk-text-field-autofill-start 0s 1ms; }
    </style><main></main></html>`;
  await page.goto(`data:text/html,${encodeURIComponent(form)}`);
  await onEveryScan(
    page,
    "const i = document.createElement('input'); i.className = 'watch'; i.setAttribute('aria-label', 'field ' + n); document.querySelector('main').append(i);",
  );
  expect(await axeViolations(page)).toEqual([]);
  expect(await scans(page)).toBe(1);
});
