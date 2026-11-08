import { expect, test, type Page } from '@playwright/test';
import { axeInThemes } from '../../../../ui/projects/paddock-e2e/src/axe.ts';
import { openApp } from './app';

// The Chat tab's composer (ADR 0023 WP-4b): text files attached through the picker, refusals in words, the files on the sent
// question, the context meter, the layout from 900 to 1440 px wide and axe over the pane with all of it on screen. Browser
// build with the mock backend; Rust's `chat_host::attach` is the reference for what is taken.

const notes = { name: 'notes.md', mimeType: 'text/markdown', buffer: Buffer.from('# Notes\nThe disk filled up at 03:10.\n') };
const photo = { name: 'photo.png', mimeType: 'image/png', buffer: Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]) };

async function chatWithFile(page: Page) {
  await openApp(page, '/playground');
  const chat = page.locator('pw-playground-chat');
  await expect(chat.getByTestId('assistant-empty')).toBeVisible();
  await chat.getByTestId('assistant-file-input').setInputFiles(notes);
  await expect(chat.getByTestId('assistant-file')).toContainText('notes.md');
  return chat;
}

test.describe('chat composer', () => {
  test('attaches a text file, sends it with the question and shows it on the sent turn; the meter follows the chat', async ({ page }) => {
    const chat = await chatWithFile(page);
    await expect(chat.getByTestId('assistant-file')).toHaveAttribute('title', 'notes.md (37 B · 2 lines)');
    await expect(chat.getByTestId('assistant-attach')).toHaveAttribute('title', /256 KiB/);
    await expect(chat.getByTestId('assistant-context')).toContainText(/of 8\.2k tokens/);
    await chat.getByTestId('assistant-input').fill('What happened?');
    await chat.getByTestId('assistant-input').press('Enter');
    await expect(chat.getByTestId('assistant-turn-file')).toContainText('notes.md');
    await expect(chat.getByTestId('assistant-file')).toHaveCount(0);
    await expect(chat.getByTestId('assistant-answer').last()).not.toBeEmpty({ timeout: 10_000 });
    await expect(chat.getByTestId('assistant-context')).toContainText(/^\d+ of 8\.2k tokens$/);
  });

  test('refuses an image in words and keeps the question usable; a removed file is not sent', async ({ page }) => {
    const chat = await chatWithFile(page);
    await chat.getByTestId('assistant-file-input').setInputFiles(photo);
    await expect(chat.getByTestId('assistant-failure')).toContainText('"photo.png" is an image');
    await chat.getByTestId('assistant-file-remove').click();
    await expect(chat.getByTestId('assistant-file')).toHaveCount(0);
    await expect(chat.getByTestId('assistant-input')).toBeFocused();
  });

  for (const size of [
    { width: 900, height: 700 },
    { width: 1440, height: 900 },
  ]) {
    test(`fits ${size.width} px: nothing scrolls sideways, the composer's controls are on screen and apart`, async ({ page }) => {
      await page.setViewportSize(size);
      const chat = await chatWithFile(page);
      await chat.getByTestId('assistant-input').fill('A question');
      const overflow = await page.evaluate(() => ({
        page: document.documentElement.scrollWidth - document.documentElement.clientWidth,
        pane: (() => {
          const el = document.querySelector('pd-assistant');
          return el ? el.scrollWidth - el.clientWidth : -1;
        })(),
      }));
      expect(overflow).toEqual({ page: 0, pane: 0 });
      const boxes = await Promise.all(['assistant-attach', 'assistant-context', 'assistant-send', 'assistant-input'].map((id) => chat.getByTestId(id).boundingBox()));
      const [attach, meter, send, input] = boxes;
      for (const b of boxes) {
        expect(b).not.toBeNull();
        expect(b!.x).toBeGreaterThanOrEqual(0);
        expect(b!.x + b!.width).toBeLessThanOrEqual(size.width);
        expect(b!.y + b!.height).toBeLessThanOrEqual(size.height);
      }
      // The remove target is at least 24 px square (review P2-9); the empty state is whole, its icon below the pane's header
      // (review P2-10: attaching files on a short window once scrolled it under the header).
      const x = await chat.getByTestId('assistant-file-remove').boundingBox();
      expect(x!.width).toBeGreaterThanOrEqual(24);
      expect(x!.height).toBeGreaterThanOrEqual(24);
      const log = await chat.getByTestId('assistant-log').boundingBox();
      const icon = await chat.locator('.empty-icon').boundingBox();
      expect(icon!.y).toBeGreaterThanOrEqual(log!.y);
      // The meter has its own room (a page-wide `.meter` style once squeezed it to 0 px wide).
      expect(meter!.width).toBeGreaterThan(100);
      // One row under the question: attach, then the meter, then Send at the right edge; nothing overlaps.
      expect(attach!.y).toBeGreaterThan(input!.y + input!.height - 1);
      expect(meter!.x).toBeGreaterThanOrEqual(attach!.x + attach!.width);
      expect(send!.x).toBeGreaterThanOrEqual(meter!.x + meter!.width);
      expect(Math.abs(attach!.y + attach!.height / 2 - (send!.y + send!.height / 2))).toBeLessThan(4);
    });
  }

  test('the pane has no axe violations with a file attached, a sent file and the meter, in light and dark', async ({ page }) => {
    const chat = await chatWithFile(page);
    await chat.getByTestId('assistant-input').fill('What happened?');
    await chat.getByTestId('assistant-input').press('Enter');
    await expect(chat.getByTestId('assistant-turn-file')).toBeVisible();
    await expect(chat.getByTestId('assistant-answer').last()).not.toBeEmpty({ timeout: 10_000 });
    await chat.getByTestId('assistant-file-input').setInputFiles({ ...notes, name: 'second.txt' });
    await expect(chat.getByTestId('assistant-file')).toBeVisible();
    expect(await axeInThemes(page, { include: ['pd-assistant'] })).toEqual([]);
  });
});
