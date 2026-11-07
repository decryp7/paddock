import { expect, test, type Page } from '@playwright/test';
import { openApp, settingsDialog } from './app';

// ADR 0023 Decision 3 (WP-3b) against the mock: the Chat bar's "Don't save this chat" (a new chat takes it to its first
// question; a saved chat asks before its saved copy is deleted) and Settings > Chats (the default, the privacy words, Delete all).

const unsaved = (page: Page) =>
  page.evaluate(() => [...(window as unknown as { __pitwallMock: { unsavedChats: Set<string> } }).__pitwallMock.unsavedChats]);

async function ask(page: Page, text: string): Promise<void> {
  const chat = page.locator('pw-playground-chat');
  await chat.getByTestId('assistant-input').fill(text);
  await chat.getByTestId('assistant-input').press('Enter');
  await expect(chat.getByTestId('assistant-answer').last()).not.toBeEmpty();
}

test.describe('saved chats', () => {
  test("Don't save: a new chat takes it; a saved chat asks first, No keeps it saved, Yes deletes the saved copy", async ({ page }) => {
    await openApp(page, '/playground');
    const chat = page.locator('pw-playground-chat');
    const toggle = chat.getByRole('switch', { name: "Don't save this chat" });
    await expect(toggle).not.toBeChecked();

    // A new chat: the switch goes with its first question, no question asked.
    await toggle.click();
    await expect(toggle).toBeChecked();
    await ask(page, 'something private');
    await expect(toggle).toBeChecked();
    await expect(page.getByRole('alertdialog')).toHaveCount(0);

    // New conversation: back to the default (saved).
    await chat.getByTestId('assistant-new').click();
    await expect(toggle).not.toBeChecked();
    await ask(page, 'keep this one');
    await expect(toggle).not.toBeChecked();

    // A saved chat: asks first. No keeps it saved.
    await toggle.click();
    let confirm = page.getByRole('alertdialog', { name: /Delete the saved copy of this chat/ });
    await expect(confirm).toBeVisible();
    await confirm.getByRole('button', { name: 'Cancel' }).click();
    await expect(confirm).toHaveCount(0);
    await expect(toggle).not.toBeChecked();
    expect(await unsaved(page)).toEqual([]);

    // Yes: the chat is no longer saved.
    await toggle.click();
    confirm = page.getByRole('alertdialog', { name: /Delete the saved copy of this chat/ });
    await confirm.getByRole('button', { name: 'Delete saved copy' }).click();
    await expect(toggle).toBeChecked();
    expect(await unsaved(page)).toHaveLength(1);

    // Off again: saved at once, nothing asked.
    await toggle.click();
    await expect(toggle).not.toBeChecked();
    await expect(page.getByRole('alertdialog')).toHaveCount(0);
    expect(await unsaved(page)).toEqual([]);
  });

  test('Settings > Chats: the privacy words, the default for new chats, and Delete all asks first', async ({ page }) => {
    await openApp(page, '/playground');
    const chat = page.locator('pw-playground-chat');
    await chat.getByTestId('chat-open-apps').click();
    const settings = settingsDialog(page);
    await settings.getByText('Chats', { exact: true }).first().click();
    await expect(settings.getByRole('heading', { name: 'Chats' })).toBeVisible();
    await expect(settings.getByTestId('chats-privacy')).toHaveText(
      "Chats are kept on this computer in your account's folder. They are not encrypted; anything that can read your files can read them.",
    );
    await expect(settings.getByTestId('chats-usage')).toHaveText('3 saved chats (47 KB).');
    const saveNew = settings.getByRole('switch', { name: 'Save new chats' });
    await expect(saveNew).toBeChecked();
    await saveNew.click();
    await expect(saveNew).not.toBeChecked();
    await settings.getByRole('button', { name: 'Close' }).click();
    // The Chat tab hears the default when a new chat starts: "Don't save" on.
    await ask(page, 'one more');
    await chat.getByTestId('assistant-new').click();
    await expect(chat.getByRole('switch', { name: "Don't save this chat" })).toBeChecked();

    await chat.getByTestId('chat-open-apps').click();
    await settingsDialog(page).getByText('Chats', { exact: true }).first().click();
    const s = settingsDialog(page);
    await s.getByTestId('chats-delete-all').click();
    const confirm = page.getByRole('alertdialog', { name: /Delete all chats/ });
    await expect(confirm).toContainText('This cannot be undone.');
    await confirm.getByRole('button', { name: 'Delete all chats' }).click();
    await expect(s.getByTestId('chats-usage')).toHaveText('No saved chats.');
    await expect(s.getByTestId('chats-delete-all')).toBeDisabled();
  });
});
