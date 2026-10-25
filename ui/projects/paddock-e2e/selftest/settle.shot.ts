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
