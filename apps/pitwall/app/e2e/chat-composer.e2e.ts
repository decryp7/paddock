import { expect, test, type Page } from '@playwright/test';
import { axeInThemes } from '../../../../ui/projects/paddock-e2e/src/axe.ts';
import { openApp } from './app';
import { gradientPng } from './png';

// The Chat tab's composer (ADR 0023 WP-4b, WP-5): text files and pictures attached through the picker, paste or drop, refusals in
// words, the files on the sent question, the context meter, the layout from 900 to 1440 px wide and axe over the pane with all of
// it on screen. Browser build with the mock backend; Rust's `chat_host::attach` and `chat_host::images` are the reference for
// what is taken.

const notes = { name: 'notes.md', mimeType: 'text/markdown', buffer: Buffer.from('# Notes\nThe disk filled up at 03:10.\n') };
const photo = { name: 'photo.png', mimeType: 'image/png', buffer: Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]) };

const cat = { name: 'cat.png', mimeType: 'image/png', buffer: gradientPng(96, 64) };

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
    await expect(chat.getByTestId('assistant-context')).toContainText(/of 7\.2k tokens/);
    await chat.getByTestId('assistant-input').fill('What happened?');
    await chat.getByTestId('assistant-input').press('Enter');
    await expect(chat.getByTestId('assistant-turn-file')).toContainText('notes.md');
    await expect(chat.getByTestId('assistant-file')).toHaveCount(0);
    await expect(chat.getByTestId('assistant-answer').last()).not.toBeEmpty({ timeout: 10_000 });
    await expect(chat.getByTestId('assistant-context')).toContainText(/^\d+ of 7\.2k tokens$/);
  });

  test('refuses an image for a model without vision in words, keeps the question usable, and a removed file is not sent', async ({ page }) => {
    const chat = await chatWithFile(page);
    await chat.getByTestId('assistant-file-input').setInputFiles(photo);
    await expect(chat.getByTestId('assistant-failure')).toContainText("Qwen3.5 4B can't read images. Choose Gemma 4 E4B to ask about an image.");
    await chat.getByTestId('assistant-file-remove').click();
    await expect(chat.getByTestId('assistant-file')).toHaveCount(0);
    await expect(chat.getByTestId('assistant-input')).toBeFocused();
  });

  test('images: the bar offers the vision model, an image shows as a thumbnail, goes with the question and shows on the sent turn', async ({ page }) => {
    await openApp(page, '/playground');
    const chat = page.locator('pw-playground-chat');
    // The refusal carries the way out; the bar says nothing about images (it stays one row at 900 px).
    await chat.getByTestId('assistant-file-input').setInputFiles(cat);
    await expect(chat.getByTestId('assistant-failure')).toContainText("Qwen3.5 4B can't read images");
    await chat.getByTestId('assistant-failure-image-action').click();
    await expect(chat.getByTestId('chat-model')).toHaveText('Gemma 4 E4B');
    await chat.getByTestId('assistant-file-input').setInputFiles(cat);
    const thumb = chat.getByTestId('assistant-file-thumb');
    await expect(thumb).toBeVisible();
    await expect(chat.getByTestId('assistant-file')).toHaveAttribute('title', /^cat\.png \(96 × 64 · \d+ KB\)$/);
    await expect.poll(() => thumb.evaluate((i: HTMLImageElement) => i.complete && i.naturalWidth)).toBe(96);
    const box = (await thumb.boundingBox())!;
    expect(box.width).toBeGreaterThanOrEqual(20);
    await chat.getByTestId('assistant-input').fill('What is in this image?');
    await chat.getByTestId('assistant-input').press('Enter');
    await expect(chat.getByTestId('assistant-turn-thumb')).toBeVisible();
    await expect(chat.getByTestId('assistant-file')).toHaveCount(0);
    await expect(chat.getByTestId('assistant-answer').last()).not.toBeEmpty({ timeout: 10_000 });
  });

  test('an image waiting for a model that cannot read it keeps Send off, with the words and the way out', async ({ page }) => {
    await openApp(page, '/playground');
    const chat = page.locator('pw-playground-chat');
    await chat.getByTestId('chat-model').click();
    await page.getByRole('option', { name: /Gemma 4 E4B/ }).click();
    await expect(page.locator('.pw-chat-model-panel')).toHaveCount(0);
    await chat.getByTestId('assistant-file-input').setInputFiles(cat);
    await expect(chat.getByTestId('assistant-file-thumb')).toBeVisible();
    await chat.getByTestId('assistant-input').fill('What is this?');
    await expect(chat.getByTestId('assistant-send')).toBeEnabled();
    // Choose a model without vision: the picture stays, Send waits, and the words say what to do.
    await chat.getByTestId('chat-model').click();
    await page.getByRole('option', { name: /Qwen3\.5 9B/ }).click();
    await expect(page.locator('.pw-chat-model-panel')).toHaveCount(0);
    await expect(chat.getByTestId('assistant-image-block')).toContainText("Qwen3.5 9B can't read images. Choose Gemma 4 E4B to ask about an image.");
    await expect(chat.getByTestId('assistant-send')).toBeDisabled();
    await expect(chat.getByTestId('assistant-image-action')).toHaveText('Use Gemma 4 E4B for images');
    await chat.getByTestId('assistant-image-action').click();
    await expect(chat.getByTestId('assistant-image-block')).toHaveCount(0);
    await expect(chat.getByTestId('assistant-send')).toBeEnabled();
  });

  test('a tray of thumbnails shows whole rows (no clipped row), says "16 images" when folded, and the bar stays on one row at 900 px', async ({ page }) => {
    await page.setViewportSize({ width: 900, height: 700 });
    await openApp(page, '/playground');
    const chat = page.locator('pw-playground-chat');
    await chat.getByTestId('chat-model').click();
    await page.getByRole('option', { name: /Gemma 4 E4B/ }).click();
    await expect(page.locator('.pw-chat-model-panel')).toHaveCount(0);
    const many = Array.from({ length: 16 }, (_, i) => ({ name: `shot-${i + 1}.png`, mimeType: 'image/png', buffer: gradientPng(40 + i, 30) }));
    await chat.getByTestId('assistant-file-input').setInputFiles(many);
    await expect(chat.getByTestId('assistant-file')).toHaveCount(16);
    await expect(chat.getByTestId('assistant-files-summary')).toHaveText('16 images');
    const tabs = (await page.getByRole('tablist').boundingBox())!;
    const bar = (await chat.getByTestId('chat-model').boundingBox())!;
    expect(bar.y - (tabs.y + tabs.height), 'the bar is one row: the pane starts right below it').toBeLessThan(60);
    // Whole rows: folded in a short pane, two rows of 24 px chips with their gap (54 px) and no third row showing.
    const rows = async () => chat.getByTestId('assistant-file').evaluateAll((els) => new Set(els.map((e) => Math.round(e.getBoundingClientRect().top))).size);
    expect(Math.round((await chat.getByTestId('assistant-files').boundingBox())!.height)).toBe(54);
    // Tall enough, it opens to two whole rows of 32 px thumbnails (72 px with its padding), the rest scrolls with the fade.
    await page.setViewportSize({ width: 1440, height: 900 });
    await expect(chat.getByTestId('assistant-files')).toHaveClass(/scrolls/);
    const tray = (await chat.getByTestId('assistant-files').boundingBox())!;
    expect(Math.round(tray.height)).toBe(72);
    const edge = tray.y + tray.height;
    const spans = await chat.getByTestId('assistant-file').evaluateAll((els) => els.map((e) => ({ top: e.getBoundingClientRect().top, bottom: e.getBoundingClientRect().bottom })));
    // Every chip is wholly inside the tray's visible height or wholly below it: no row is cut.
    expect(spans.filter((c) => c.bottom > edge + 0.5 && c.top < edge - 0.5)).toEqual([]);
    expect(await rows()).toBeGreaterThan(2);
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
      const empty = await chat.getByTestId('assistant-empty').boundingBox();
      expect(empty!.y).toBeGreaterThanOrEqual(log!.y);
      // The log keeps its room under the composer (review 2 P2-A).
      expect(log!.height).toBeGreaterThanOrEqual(119);
      // The meter has its own room (a page-wide `.meter` style once squeezed it to 0 px wide).
      expect(meter!.width).toBeGreaterThan(100);
      // One row under the question: attach, then the meter, then Send at the right edge; nothing overlaps.
      expect(attach!.y).toBeGreaterThan(input!.y + input!.height - 1);
      expect(meter!.x).toBeGreaterThanOrEqual(attach!.x + attach!.width);
      expect(send!.x).toBeGreaterThanOrEqual(meter!.x + meter!.width);
      expect(Math.abs(attach!.y + attach!.height / 2 - (send!.y + send!.height / 2))).toBeLessThan(4);
    });
  }

  test('on a short window many files fold to two rows behind an "N files" chip, and the log keeps its room (review 2 P2-A)', async ({ page }) => {
    await page.setViewportSize({ width: 900, height: 700 });
    const chat = await chatWithFile(page);
    // The bar above has "Connected apps…": the empty chat does not repeat it on a short window.
    await expect(chat.getByTestId('assistant-empty-action')).toHaveCount(0);
    const many = Array.from({ length: 15 }, (_, i) => ({ name: `part-${i + 1}.txt`, mimeType: 'text/plain', buffer: Buffer.from(`part ${i + 1}\n`) }));
    await chat.getByTestId('assistant-file-input').setInputFiles(many);
    await expect(chat.getByTestId('assistant-file')).toHaveCount(16);
    const summary = chat.getByTestId('assistant-files-summary');
    await expect(summary).toHaveText('16 files');
    await expect(summary).toHaveAttribute('aria-expanded', 'false');
    const folded = (await chat.getByTestId('assistant-files').boundingBox())!;
    expect(folded.height).toBeLessThanOrEqual(56);
    expect((await chat.getByTestId('assistant-log').boundingBox())!.height).toBeGreaterThanOrEqual(119);
    // The question and Send are inside the pane (its frame clips what overflows), opened or folded, with a refusal on screen.
    const inPane = async (): Promise<void> => {
      const pane = (await chat.locator('pd-assistant').boundingBox())!;
      for (const id of ['assistant-input', 'assistant-send']) {
        const b = (await chat.getByTestId(id).boundingBox())!;
        expect(b.y + b.height).toBeLessThanOrEqual(pane.y + pane.height);
      }
    };
    await inPane();
    await chat.getByTestId('assistant-file-input').setInputFiles({ name: 'one-more.txt', mimeType: 'text/plain', buffer: Buffer.from('x\n') });
    await expect(chat.getByTestId('assistant-failure')).toContainText('at most 16 attachments');
    await inPane();
    await summary.click();
    await expect(summary).toHaveAttribute('aria-expanded', 'true');
    expect((await chat.getByTestId('assistant-files').boundingBox())!.height).toBeGreaterThan(folded.height);
    await inPane();
    // Opened, it still scrolls and says so with a fade at its foot (review 3 P2-5); the log keeps its floor (P2-3).
    await expect(chat.getByTestId('assistant-files')).toHaveClass(/scrolls/);
    expect((await chat.getByTestId('assistant-log').boundingBox())!.height).toBeGreaterThanOrEqual(55);
    await summary.click();
    await expect(summary).toHaveAttribute('aria-expanded', 'false');
    // A tall window shows every row, with no summary.
    await page.setViewportSize({ width: 1440, height: 900 });
    await expect(summary).toHaveCount(0);
  });

  test('in a very short pane the log keeps a floor of about two lines and the question and Send stay in the pane (review 3 P2-3)', async ({ page }) => {
    await page.setViewportSize({ width: 900, height: 640 });
    const chat = await chatWithFile(page);
    await chat.getByTestId('assistant-file-input').setInputFiles(photo);
    await expect(chat.getByTestId('assistant-failure')).toBeVisible();
    const pane = (await chat.locator('pd-assistant').boundingBox())!;
    expect(pane.height).toBeLessThan(440);
    await expect(chat.locator('pd-assistant')).toHaveClass(/short/);
    expect((await chat.getByTestId('assistant-log').boundingBox())!.height).toBeGreaterThanOrEqual(55);
    for (const id of ['assistant-input', 'assistant-send']) {
      const b = (await chat.getByTestId(id).boundingBox())!;
      expect(b.y + b.height, id).toBeLessThanOrEqual(pane.y + pane.height);
      expect(b.y + b.height, `${id} on screen`).toBeLessThanOrEqual(640);
    }
  });

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
