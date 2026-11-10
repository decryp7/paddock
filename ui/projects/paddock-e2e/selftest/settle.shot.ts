import { expect, test } from '@playwright/test';
import { settleTransitions } from '../src/index.ts';

// axeViolations waits for transitions: a colour changing over 400 ms is read at its end, an endless animation does not hold it up.
test('settleTransitions waits for a colour transition and ignores an endless animation', async ({ page }) => {
  const body = '<style>@keyframes spin{to{transform:rotate(1turn)}} #t{color:rgb(0,0,0);transition:color 400ms linear} #t.on{color:rgb(200,0,0)} #s{animation:spin 1s linear infinite}</style><p id="t">text</p><i id="s">o</i>';
  await page.goto(`data:text/html,${encodeURIComponent(body)}`);
  await page.evaluate(() => document.getElementById('t')?.classList.add('on'));
  const during = await page.evaluate(() => getComputedStyle(document.getElementById('t') as HTMLElement).color);
  expect(during).not.toBe('rgb(200, 0, 0)');
  await settleTransitions(page);
  expect(await page.evaluate(() => getComputedStyle(document.getElementById('t') as HTMLElement).color)).toBe('rgb(200, 0, 0)');
  await settleTransitions(page);
});

// Pitwall's dashboard restarts a 1 s width transition on every countdown tick, so "wait until the transitions end" never
// ends (axe took 1-23 s per theme there). Settling finishes them instead: the end state at once, however often they restart.
test('settleTransitions returns at once while a transition restarts on every tick', async ({ page }) => {
  const body = '<style>#w{width:10px;height:4px;background:#000;transition:width 1s linear}</style><div id="w"></div>';
  await page.goto(`data:text/html,${encodeURIComponent(body)}`);
  await page.evaluate(() => {
    let n = 0;
    setInterval(() => ((document.getElementById('w') as HTMLElement).style.width = `${10 + (++n % 2) * 100}px`), 300);
  });
  await page.waitForFunction(() => document.getAnimations().length > 0);
  const t = Date.now();
  await settleTransitions(page);
  expect(Date.now() - t, 'settling waited for transitions that restart').toBeLessThan(800);
});
