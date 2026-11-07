import { expect, test } from '@playwright/test';
import { openApp, openSettings, settingsDialog } from './app';

// ADR 0023 WP-9a against the mock: the Chat tab's picker and apps line, Settings > Connected apps, and the key dialog.
test.describe('connected apps and the chat model', () => {
  test('connect Dyno with a pasted key from the Chat tab, see its tools, then forget it', async ({ page }) => {
    await openApp(page, '/playground');
    const chat = page.locator('pw-playground-chat');
    await expect(chat.getByTestId('chat-apps')).toHaveText('No app connected: the chat answers without tools.');
    await expect(chat.getByTestId('chat-model')).toContainText('Qwen3.5 4B');
    await chat.getByTestId('chat-open-apps').click();
    const settings = settingsDialog(page);
    await expect(settings.getByRole('heading', { name: 'Connected apps' })).toBeVisible();
    const dyno = settings.getByTestId('connected-app');
    await expect(dyno.getByTestId('connected-app-state')).toHaveText('Not set up');

    // A wrong key: kept, refused by Dyno, and the page says what to do.
    await dyno.getByRole('button', { name: 'Connect…' }).click();
    let form = page.getByRole('dialog', { name: 'Connect Dyno' });
    await expect(form.getByRole('heading', { name: "Paste Dyno's AI access key" })).toBeVisible();
    await expect(form.getByTestId('dialog-action-connect')).toBeDisabled();
    await form.locator('#pw-mcp-key').fill('not a key');
    await expect(form.getByTestId('dialog-action-connect')).toBeDisabled();
    await expect(form.getByTestId('mcp-key-not-a-key')).toHaveText('An AI access key starts with pmk_.');
    await form.locator('#pw-mcp-key').fill('pmk_wrong');
    await form.getByTestId('dialog-action-connect').click();
    await expect(dyno.getByTestId('connected-app-problem')).toContainText('refused the key');
    // Only a new key helps: no Refresh while the key is refused.
    await dyno.getByRole('button', { name: 'More actions for Dyno' }).click();
    await expect(page.getByRole('menuitem', { name: 'Refresh tools' })).toHaveCount(0);
    await page.getByRole('menu').press('Escape');
    await expect(page.getByRole('menu')).toHaveCount(0);

    // The right key: connected, its tools listed by kind.
    await dyno.getByRole('button', { name: 'Replace key…' }).click();
    form = page.getByRole('dialog', { name: 'Connect Dyno' });
    await form.locator('#pw-mcp-key').fill('pmk_good');
    await form.getByTestId('dialog-action-connect').click();
    await expect(dyno.getByTestId('connected-app-state')).toHaveText('Connected: 6 tools');
    await expect(dyno.getByText('Action: asks every time')).toBeVisible();
    await expect(dyno.getByText('Top processes')).toBeVisible();
    await dyno.getByRole('button', { name: 'More actions for Dyno' }).click();
    await expect(page.getByRole('menuitem', { name: 'Refresh tools' })).toBeVisible();
    await page.getByRole('menu').press('Escape');
    await expect(page.getByRole('menu')).toHaveCount(0);

    // Dyno quits: the row says so; Open Dyno starts it and the chat connects again.
    await page.evaluate(() => (window as unknown as { __pitwallMock?: { dynoQuit(): void } }).__pitwallMock?.dynoQuit());
    await expect(chat.getByTestId('chat-apps')).toHaveText('Dyno is reconnecting: the chat answers without its tools for now.');
    await dyno.getByRole('button', { name: 'More actions for Dyno' }).click();
    await expect(page.getByRole('menuitem', { name: 'Refresh tools' })).toHaveCount(0);
    await page.getByRole('menu').press('Escape');
    await expect(page.getByRole('menu')).toHaveCount(0);
    await expect(dyno.getByTestId('connected-app-problem')).toContainText('Trying again when Dyno is back');
    await dyno.getByRole('button', { name: 'Open Dyno' }).click();
    await expect(dyno.getByTestId('connected-app-state')).toHaveText('Connected: 6 tools');
    await expect(dyno.getByTestId('connected-app-opening')).toHaveCount(0);

    // One tool is always allowed: Ask again forgets just that one.
    await dyno.getByRole('button', { name: 'Ask again before System summary' }).click();
    await expect(dyno.getByText('Always', { exact: true })).toHaveCount(0);
    await expect(dyno.getByText('No tool is always allowed yet.')).toBeVisible();

    // The Chat tab is another window: it hears the connect and says so without being opened again.
    await expect(chat.getByTestId('chat-apps')).toHaveText('Tools from Dyno (6).');

    // Forget asks first, naming what goes.
    await dyno.getByRole('button', { name: 'More actions for Dyno' }).click();
    await page.getByRole('menuitem', { name: 'Forget Dyno…' }).click();
    const ask = page.getByRole('alertdialog', { name: /Forget Dyno/ });
    await expect(ask).toContainText("Pitwall removes Dyno's key and its connection.");
    await ask.getByRole('button', { name: 'Forget Dyno' }).click();
    await expect(dyno.getByTestId('connected-app-state')).toHaveText('Not set up');
    await expect(chat.getByTestId('chat-apps')).toHaveText('No app connected: the chat answers without tools.');
  });

  // Round-4 review P1-1 and P1-2: a problem Open meets is said once, with nothing pulsing, and Open stays available; a Dyno too
  // old for Pitwall is not "reconnecting", so the Refresh tools its words point to is in the menu.
  test('not installed and too old are said once, and the actions they name are there', async ({ page }) => {
    type Hooks = { dynoQuit(): void; dynoUninstalled(): void; dynoTooOld(): void };
    const hook = (name: keyof Hooks) => page.evaluate((n) => (window as unknown as { __pitwallMock: Hooks }).__pitwallMock[n](), name);
    await openApp(page, '/playground');
    const chat = page.locator('pw-playground-chat');
    await chat.getByTestId('chat-open-apps').click();
    const settings = settingsDialog(page);
    const dyno = settings.getByTestId('connected-app');
    await dyno.getByRole('button', { name: 'Connect…' }).click();
    const form = page.getByRole('dialog', { name: 'Connect Dyno' });
    await form.locator('#pw-mcp-key').fill('pmk_good');
    await form.getByTestId('dialog-action-connect').click();
    await expect(dyno.getByTestId('connected-app-state')).toHaveText('Connected: 6 tools');

    await hook('dynoQuit');
    await hook('dynoUninstalled');
    await dyno.getByRole('button', { name: 'Open Dyno' }).click();
    await expect(dyno.getByTestId('connected-app-problem')).toHaveText("Dyno isn't installed on this computer. Install Dyno, then choose Open Dyno.");
    await expect(dyno.getByText("Dyno isn't installed on this computer.", { exact: false })).toHaveCount(1);
    await expect(dyno.getByTestId('connected-app-opening')).toHaveCount(0);
    await expect(dyno.locator('.pulse')).toHaveCount(0);
    await expect(dyno.getByRole('button', { name: 'Open Dyno' })).toBeEnabled();
    await expect(chat.getByTestId('chat-apps')).toHaveText("Dyno isn't installed: the chat answers without its tools.");

    await hook('dynoTooOld');
    await expect(dyno.getByTestId('connected-app-problem')).toHaveText('Dyno is too old for AI access from Pitwall. Update Dyno, then choose Refresh tools.');
    await expect(chat.getByTestId('chat-apps')).toHaveText('Dyno needs an update: the chat answers without its tools.');
    await dyno.getByRole('button', { name: 'More actions for Dyno' }).click();
    await expect(page.getByRole('menuitem', { name: 'Refresh tools' })).toBeVisible();
    await page.getByRole('menu').press('Escape');
    await expect(page.getByRole('menu')).toHaveCount(0);
  });

  // Round-4 review P2: a tool's name is never cut for its status, and every tool row is as tall as the others.
  test('tool rows keep the name whole and share one height', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await openApp(page, '/playground');
    await page.locator('pw-playground-chat').getByTestId('chat-open-apps').click();
    const dyno = settingsDialog(page).getByTestId('connected-app');
    await dyno.getByRole('button', { name: 'Connect…' }).click();
    const form = page.getByRole('dialog', { name: 'Connect Dyno' });
    await form.locator('#pw-mcp-key').fill('pmk_good');
    await form.getByTestId('dialog-action-connect').click();
    await expect(dyno.getByTestId('connected-app-state')).toHaveText('Connected: 6 tools');
    await expect(dyno.getByRole('button', { name: 'Ask again before System summary' })).toBeVisible();
    const rows = await dyno.locator('.tools li').evaluateAll((lis) =>
      lis.map((li) => {
        const name = li.querySelector('.tool') as HTMLElement;
        return { height: Math.round(li.getBoundingClientRect().height), cut: name.scrollWidth > name.clientWidth, name: name.textContent };
      }),
    );
    expect(rows.filter((r) => r.cut), 'no name is cut').toEqual([]);
    expect(new Set(rows.map((r) => r.height)).size, JSON.stringify(rows)).toBe(1);
  });

  for (const theme of ['light', 'dark'] as const) {
  test(`the picker lists models that cannot use tools as disabled, with the reason, readable (${theme})`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: theme });
    await openApp(page, '/playground');
    const chat = page.locator('pw-playground-chat');
    await chat.getByTestId('chat-model').click();
    const gemma = page.getByRole('option', { name: /Gemma 4 12B/ });
    await expect(gemma).toHaveAttribute('aria-disabled', 'true');
    await expect(gemma).toContainText("Can't use tools");
    await expect(page.getByTestId('chat-model-note').filter({ hasText: 'chat template has no tool calls' })).toHaveCount(1);
    await expect(page.getByRole('option', { name: /Qwen3.8 27B/ })).toHaveAttribute('aria-disabled', 'true');
    // Disabled but readable: the reason is the muted text colour at full opacity, at least 4.5:1 on the panel.
    const measure = () => gemma.locator('.why').evaluate((el) => {
      const rgb = (c: string) => (c.match(/[\d.]+/g) ?? []).slice(0, 3).map(Number);
      const lum = ([r, g, b]: number[]) => {
        const f = (v: number) => ((v /= 255) <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
        return 0.2126 * f(r!) + 0.7152 * f(g!) + 0.0722 * f(b!);
      };
      let bg: Element | null = el;
      let back = 'rgba(0, 0, 0, 0)';
      while (bg && /rgba\(0, 0, 0, 0\)|transparent/.test(back)) {
        back = getComputedStyle(bg).backgroundColor;
        bg = bg.parentElement;
      }
      let opacity = 1;
      for (let e: Element | null = el; e; e = e.parentElement) opacity *= Number(getComputedStyle(e).opacity);
      const [a, b] = [lum(rgb(getComputedStyle(el).color)), lum(rgb(back))];
      return { ratio: (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05), opacity };
    });
    // The panel fades in: measure once it is fully shown (a condition, not a sleep).
    await expect.poll(async () => (await measure()).opacity).toBe(1);
    expect((await measure()).ratio).toBeGreaterThanOrEqual(4.5);
    // Round-4 review P2: more is below, so the list fades at its bottom edge; scrolled to the end, the last row is clear of it.
    const panel = page.locator('.pw-chat-model-panel');
    const fade = () => panel.evaluate((el) => ({ more: el.scrollHeight > el.clientHeight, mask: getComputedStyle(el).maskImage || getComputedStyle(el).webkitMaskImage }));
    expect((await fade()).more).toBe(true);
    expect((await fade()).mask).toContain('linear-gradient');
    const clear = await panel.evaluate((el) => {
      el.scrollTop = el.scrollHeight;
      const options = el.querySelectorAll('mat-option');
      const last = options[options.length - 1]!.getBoundingClientRect();
      const box = el.getBoundingClientRect();
      const fadePx = parseFloat(getComputedStyle(el).getPropertyValue('--pw-fade')) || 0;
      return { fadePx, gap: box.bottom - last.bottom };
    });
    expect(clear.fadePx).toBeGreaterThan(0);
    expect(clear.gap).toBeGreaterThanOrEqual(clear.fadePx - 1);
    await panel.evaluate((el) => (el.scrollTop = 0));
    await page.getByRole('option', { name: /Qwen3.5 9B/ }).click();
    await expect(chat.getByTestId('chat-model')).toContainText('Qwen3.5 9B');
  });
}
});

test.describe('a build without the chat', () => {
  test('has no Connected apps page in Settings', async ({ page }) => {
    await openSettings(page, 'mock-fast&mock-static&mock-nochat');
    const settings = settingsDialog(page);
    await expect(settings.getByText('Tray popup', { exact: true }).first()).toBeVisible();
    await expect(settings.getByText('Connected apps', { exact: true })).toHaveCount(0);
  });

  test('a build with it lists the page', async ({ page }) => {
    await openSettings(page);
    await expect(settingsDialog(page).getByText('Connected apps', { exact: true }).first()).toBeVisible();
  });
});
