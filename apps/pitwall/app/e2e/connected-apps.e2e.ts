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
    await expect(form.getByTestId('dialog-action-connect')).toBeDisabled();
    await form.getByLabel('AI access key').fill('not a key');
    await expect(form.getByTestId('dialog-action-connect')).toBeDisabled();
    await form.getByLabel('AI access key').fill('pmk_wrong');
    await form.getByTestId('dialog-action-connect').click();
    await expect(dyno.getByTestId('connected-app-problem')).toContainText('refused the key');

    // The right key: connected, its tools listed by kind.
    await dyno.getByRole('button', { name: 'Replace key…' }).click();
    form = page.getByRole('dialog', { name: 'Connect Dyno' });
    await form.getByLabel('AI access key').fill('pmk_good');
    await form.getByTestId('dialog-action-connect').click();
    await expect(dyno.getByTestId('connected-app-state')).toHaveText('Connected: 6 tools');
    await expect(dyno.getByText('Action: asks every time')).toBeVisible();
    await dyno.getByRole('button', { name: 'Forget' }).click();
    await expect(dyno.getByTestId('connected-app-state')).toHaveText('Not set up');
  });

  test('the picker lists models that cannot use tools as disabled, with the reason', async ({ page }) => {
    await openApp(page, '/playground');
    const chat = page.locator('pw-playground-chat');
    await chat.getByTestId('chat-model').click();
    const gemma = page.getByRole('option', { name: /Gemma 4 12B/ });
    await expect(gemma).toHaveAttribute('aria-disabled', 'true');
    await expect(gemma).toContainText("Can't use tools");
    await page.getByRole('option', { name: /Qwen3.5 9B/ }).click();
    await expect(chat.getByTestId('chat-model')).toContainText('Qwen3.5 9B');
  });
});
