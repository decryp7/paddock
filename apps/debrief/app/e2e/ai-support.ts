import { expect, Locator, Page } from '@playwright/test';
import { gridRows, openAppWith } from './mock-url';
import { expectStreamDone } from '../../../../ui/projects/paddock-e2e/src/ready.ts';

/** Opens the app with the scripted AI mock (fast streaming) plus `extra` switches, and waits for the grid. */
export async function openAiApp(page: Page, extra = ''): Promise<void> {
  await openAppWith(page, `?mockAiStepMs=5${extra ? `&${extra}` : ''}`);
  await expect(gridRows(page).first()).toBeVisible();
}

/** Lets everything the mocks hold (`mockHold`, `mockJobsHold`) go on. */
export async function releaseMockHolds(page: Page): Promise<void> {
  await page.evaluate(() => (globalThis as unknown as { __debriefMockRelease: () => void }).__debriefMockRelease());
}

/** Opens the assistant chat from the palette and returns the panel. */
export async function openChat(page: Page): Promise<Locator> {
  await runCommand(page, 'AI: Open the assistant chat');
  const panel = page.locator('debrief-ai-chat-panel');
  await expect(panel.getByRole('textbox', { name: 'Ask about the logs' })).toBeEnabled();
  return panel;
}

/**
 * Sends a question and waits until its turn is drawn: the stream has started (the chat log is `aria-busy` from here to the end of the answer). The
 * answer streams on; follow it with `waitAnswered` before pressing anything in it (a card above which text still arrives moves).
 */
export async function ask(panel: Locator, question: string): Promise<void> {
  const turns = panel.locator('.turn');
  const before = await turns.count();
  await panel.getByRole('textbox', { name: 'Ask about the logs' }).fill(question);
  await panel.getByRole('button', { name: 'Send', exact: true }).click();
  await expect(turns, 'the question is drawn as a turn').toHaveCount(before + 1);
}

/** Waits until the answer has finished streaming (call it after `ask`, which waits for the stream to start). */
export async function waitAnswered(panel: Locator): Promise<void> {
  await expectStreamDone(panel.getByRole('log', { name: 'Conversation' }));
}

/** Right-clicks the first grid row and returns the context menu. */
export async function rowMenu(page: Page): Promise<Locator> {
  await gridRows(page).first().click({ button: 'right' });
  const menu = page.getByRole('menu', { name: 'Actions' });
  await expect(menu).toBeVisible();
  return menu;
}

/** Runs a palette command by title (options are looked up in the palette only: the chat has <option>s too). */
export async function runCommand(page: Page, title: string): Promise<void> {
  await page.keyboard.press('ControlOrMeta+k');
  const input = page.getByRole('combobox', { name: 'Type a command' });
  await expect(input).toBeFocused();
  await input.fill(title);
  await expect(page.locator('debrief-command-palette').getByRole('option').first()).toContainText(title);
  await page.keyboard.press('Enter');
  await expect(input).toHaveCount(0);
}

/** Waits for the toast of a background AI answer ("Explanation ready") and opens it with View. */
export async function viewAnswer(page: Page, title: string): Promise<void> {
  const toast = page.getByTestId('notification').filter({ hasText: title });
  await expect(toast).toBeVisible({ timeout: 15_000 });
  await toast.getByTestId('notification-action-primary').click();
}

/** The dialog that shows an AI answer (opened by View). */
export function narrativeDialog(page: Page): Locator {
  return page.getByRole('dialog').filter({ has: page.getByTestId('ai-narrative') });
}
