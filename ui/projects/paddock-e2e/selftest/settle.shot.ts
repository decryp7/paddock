import { expect, test } from '@playwright/test';
import { axeViolations, settleTransitions } from '../src/index.ts';

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

// A Material tooltip opens 500 ms after the pointer rests on its host and fades in; a click leaves the pointer there. axe then
// scans a tooltip that is half faded in (run 7872: color-contrast on .mat-mdc-tooltip-surface). axeViolations must not leave a
// hover behind: no pointer on the host and no tooltip timer pending when it returns (a timer that is armed fires 500 ms later).
test('axeViolations leaves no hover tooltip pending after a click', async ({ page }) => {
  const body = `<style>@keyframes fade{from{opacity:0}to{opacity:1}} #tip{position:fixed;left:200px;top:10px;color:#ccc;background:#fff;animation:fade 200ms linear}</style>
    <button id="b" style="margin:50px;padding:20px">item</button>`;
  await page.goto(`data:text/html,${encodeURIComponent(body)}`);
  await page.evaluate(() => {
    const b = document.getElementById('b') as HTMLElement;
    let timer = 0;
    (window as unknown as { armed: () => boolean }).armed = () => timer !== 0;
    b.addEventListener('mouseenter', () => {
      timer = window.setTimeout(() => {
        timer = 0;
        const tip = document.createElement('div');
        tip.id = 'tip';
        tip.textContent = 'low contrast tip';
        document.body.append(tip);
      }, 500);
    });
    b.addEventListener('mouseleave', () => {
      clearTimeout(timer);
      timer = 0;
    });
  });
  await page.click('#b');
  expect(await page.evaluate(() => (window as unknown as { armed: () => boolean }).armed()), 'the click arms the tooltip').toBe(true);
  await axeViolations(page, { idsOnly: true });
  expect(await page.evaluate(() => (window as unknown as { armed: () => boolean }).armed()), 'a tooltip timer is still pending').toBe(false);
  expect(await page.evaluate(() => document.getElementById('b')?.matches(':hover'))).toBe(false);
});
