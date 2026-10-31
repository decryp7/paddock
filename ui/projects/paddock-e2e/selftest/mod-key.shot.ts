import { expect, test } from '@playwright/test';
import { pageModKey, pressMod } from '../src/index.ts';

// Regression (CI run 7134): WebKit on a Linux runner emulates a Mac (`devices['Desktop Safari']`), so the app binds `Mod` to
// Cmd, while Playwright's `ControlOrMeta` follows the machine the tests run on and pressed Ctrl. Here the page claims the
// platform the host is not, which is that mismatch on any machine; `pressMod` must press what the page binds.
const hostIsMac = process.platform === 'darwin';
const pageIsMac = !hostIsMac;
const body = '<p id="k"></p><script>addEventListener("keydown", (e) => { if (e.key.toLowerCase() === "p") document.getElementById("k").textContent = (e.metaKey ? "Meta+" : "") + (e.ctrlKey ? "Control+" : "") + (e.shiftKey ? "Shift+" : "") + "P"; });</script>';

test('pressMod presses the modifier the page binds Mod to, not the host’s', async ({ page }) => {
  await page.addInitScript((mac: boolean) => {
    Object.defineProperty(Navigator.prototype, 'platform', { get: () => (mac ? 'MacIntel' : 'Linux x86_64') });
    Object.defineProperty(Navigator.prototype, 'userAgent', { get: () => (mac ? 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)' : 'Mozilla/5.0 (X11; Linux x86_64)') });
  }, pageIsMac);
  await page.goto(`data:text/html,${encodeURIComponent(body)}`);
  const mod = pageIsMac ? 'Meta' : 'Control';
  expect(await pageModKey(page)).toBe(mod);
  await pressMod(page, 'Mod+Shift+P');
  await expect(page.locator('#k')).toHaveText(`${mod}+Shift+P`);
});

test('pressMod on a locator presses the page’s modifier on that element, with any key combination', async ({ page }) => {
  await page.addInitScript((mac: boolean) => {
    Object.defineProperty(Navigator.prototype, 'platform', { get: () => (mac ? 'MacIntel' : 'Linux x86_64') });
    Object.defineProperty(Navigator.prototype, 'userAgent', { get: () => (mac ? 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)' : 'Mozilla/5.0 (X11; Linux x86_64)') });
  }, pageIsMac);
  const field = '<input id="f"><p id="k"></p><script>f.addEventListener("keydown", (e) => { k.textContent = (e.metaKey ? "Meta+" : "") + (e.ctrlKey ? "Control+" : "") + (e.altKey ? "Alt+" : "") + e.key; });</script>';
  await page.goto(`data:text/html,${encodeURIComponent(field)}`);
  const mod = pageIsMac ? 'Meta' : 'Control';
  await pressMod(page.locator('#f'), 'Mod+Alt+Enter');
  await expect(page.locator('#k')).toHaveText(`${mod}+Alt+Enter`);
});
