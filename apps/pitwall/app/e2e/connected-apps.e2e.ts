import { expect, test } from '@playwright/test';
import { openApp, settingsDialog } from './app';

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
    await expect(dyno.getByRole('button', { name: 'Refresh tools' })).toHaveCount(0);

    // The right key: connected, its tools listed by kind.
    await dyno.getByRole('button', { name: 'Replace key…' }).click();
    form = page.getByRole('dialog', { name: 'Connect Dyno' });
    await form.locator('#pw-mcp-key').fill('pmk_good');
    await form.getByTestId('dialog-action-connect').click();
    await expect(dyno.getByTestId('connected-app-state')).toHaveText('Connected: 6 tools');
    await expect(dyno.getByText('Action: asks every time')).toBeVisible();
    await expect(dyno.getByText('Top processes')).toBeVisible();
    await expect(dyno.getByRole('button', { name: 'Refresh tools' })).toBeVisible();

    // Dyno quits: the row says so; Open Dyno starts it and the chat connects again.
    await page.evaluate(() => (window as unknown as { __pitwallMock?: { dynoQuit(): void } }).__pitwallMock?.dynoQuit());
    await expect(dyno.getByTestId('connected-app-problem')).toContainText('Trying again when Dyno is back');
    await dyno.getByRole('button', { name: 'Open Dyno' }).click();
    await expect(dyno.getByTestId('connected-app-state')).toHaveText('Connected: 6 tools');
    await expect(dyno.getByTestId('connected-app-opening')).toHaveCount(0);

    // One tool is always allowed: Ask again forgets just that one.
    await dyno.getByRole('button', { name: 'Ask again before System summary' }).click();
    await expect(dyno.getByText('Always allowed', { exact: true })).toHaveCount(0);
    await expect(dyno.getByText('No tool is always allowed yet.')).toBeVisible();

    // The Chat tab is another window: it hears the connect and says so without being opened again.
    await expect(chat.getByTestId('chat-apps')).toHaveText('Tools from Dyno (6).');

    // Forget asks first, naming what goes.
    await dyno.getByRole('button', { name: 'Forget Dyno…' }).click();
    const ask = page.getByRole('alertdialog', { name: /Forget Dyno/ });
    await expect(ask).toContainText("Pitwall removes Dyno's key and its connection.");
    await ask.getByRole('button', { name: 'Forget Dyno' }).click();
    await expect(dyno.getByTestId('connected-app-state')).toHaveText('Not set up');
    await expect(chat.getByTestId('chat-apps')).toHaveText('No app connected: the chat answers without tools.');
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
    await expect(page.getByTestId('chat-model-note')).toContainText('chat template has no tool calls');
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
    await page.getByRole('option', { name: /Qwen3.5 9B/ }).click();
    await expect(chat.getByTestId('chat-model')).toContainText('Qwen3.5 9B');
  });
}
});
