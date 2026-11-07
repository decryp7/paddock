import { expect, test } from '@playwright/test';
import { openApp } from './app';

test.describe('playground', () => {
  test('opens on Chat: an answer streams in the assistant pane, a tool call shows its card, Stop ends a turn', async ({ page }) => {
    await openApp(page, '/playground');
    await expect(page.getByRole('tab', { name: 'Chat' })).toHaveAttribute('aria-selected', 'true');
    const chat = page.locator('pw-playground-chat');
    await expect(chat.getByTestId('assistant-empty')).toBeVisible();
    await chat.getByTestId('assistant-input').fill('/tool things');
    await chat.getByTestId('assistant-input').press('Enter');
    await expect(chat.getByTestId('assistant-tool-card')).toContainText('find');
    await expect(chat.getByTestId('assistant-answer').last()).toContainText('Found item 7', { timeout: 10_000 });
    await chat.getByTestId('assistant-input').fill('/slow');
    await chat.getByTestId('assistant-input').press('Enter');
    await chat.getByTestId('assistant-stop').click();
    await expect(chat.getByTestId('assistant-stopped')).toBeVisible();
  });

  test('Compare streams an answer from both models with stats, and keeps the chat when switching back', async ({ page }) => {
    await openApp(page, '/playground');
    const chat = page.locator('pw-playground-chat');
    await chat.getByTestId('assistant-input').fill('hello');
    await chat.getByTestId('assistant-input').press('Enter');
    await expect(chat.getByTestId('assistant-answer').last()).not.toBeEmpty();
    await page.getByRole('tab', { name: 'Compare' }).click();
    const compare = page.getByTestId('pg-compare');
    await compare.getByLabel('Message').fill('Hello there');
    await compare.getByRole('button', { name: 'Send' }).click();
    await expect(compare.locator('.turn.bot .text').first()).not.toBeEmpty({ timeout: 10_000 });
    await expect(compare.locator('.stats')).toHaveCount(2, { timeout: 15_000 });
    await page.getByRole('tab', { name: 'Chat' }).click();
    await expect(chat.getByTestId('assistant-user')).toContainText('hello');
  });

  test('a build without the chat is the Playground as before: no tabs, the Compare toggle', async ({ page }) => {
    await openApp(page, '/playground', 'mock-fast&mock-static&mock-nochat');
    await expect(page.getByLabel('Message')).toBeVisible();
    await expect(page.getByRole('tab')).toHaveCount(0);
    await expect(page.locator('pw-playground-chat')).toHaveCount(0);
    await page.getByRole('button', { name: 'Compare' }).click();
    await expect(page.locator('.pane')).toHaveCount(2);
  });
});
