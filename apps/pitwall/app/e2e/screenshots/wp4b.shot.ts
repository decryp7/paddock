import { expect, test, type Locator, type Page } from '@playwright/test';
import { mkdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

// The Chat tab's composer of ADR 0023 WP-4b, for the independent review (rule 22): files attached and sent, a refusal, the
// context meter at each level, push-to-talk recording (browser build only: the app has no speech input until WP-7) and the
// drop highlight; 1440x900 and 900x700, light and dark, @2x; each as the whole window and the pane's composer up close.
// Only on request: `WP4B_SHOTS=1 npx playwright test -c e2e/screenshots/playwright.config.ts e2e/screenshots/wp4b.shot.ts`
// (`WP4B_OUT` names another folder). The sheet is index.html next to the pictures (.angular/ is ignored by git).
const OUT = process.env['WP4B_OUT'] ? resolve(process.env['WP4B_OUT']) : resolve(__dirname, '../../.angular/wp4b-shots');
const Q = 'mock-fast&mock-static';

const notes = { name: 'disk-report.md', mimeType: 'text/markdown', buffer: Buffer.from('# Disk report\nThe data volume filled up at 03:10.\nBackups paused.\n') };
const code = { name: 'cleanup.rs', mimeType: 'text/plain', buffer: Buffer.from('fn main() {\n    prune_snapshots(7);\n}\n'.repeat(14)) };
const photo = { name: 'screenshot.png', mimeType: 'image/png', buffer: Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]) };

const shots: { file: string; group: string; label: string }[] = [];

async function shot(page: Page, name: string, group: string, label: string): Promise<void> {
  mkdirSync(OUT, { recursive: true });
  const scheme = await page.evaluate(() => (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'));
  const width = page.viewportSize()?.width ?? 0;
  const file = `${name}-${width}-${scheme}.png`;
  await page.screenshot({ path: `${OUT}/${file}`, animations: 'disabled' });
  shots.push({ file, group, label: `${label} (${width} px, ${scheme})` });
  // The composer up close: the pane's lower part, from the last turn's end to its bottom edge.
  const pane = page.locator('pd-assistant');
  const box = await pane.boundingBox();
  if (box) {
    const height = Math.min(box.height, 260);
    const close = `${name}-${width}-${scheme}-composer.png`;
    await page.screenshot({ path: `${OUT}/${close}`, animations: 'disabled', clip: { x: box.x, y: box.y + box.height - height, width: box.width, height } });
    shots.push({ file: close, group, label: `${label}, composer up close (${width} px, ${scheme})` });
  }
}

async function chat(page: Page, query = Q): Promise<Locator> {
  await page.goto(`/?${query}#/playground`);
  const pane = page.locator('pw-playground-chat');
  await expect(pane.getByTestId('assistant-empty')).toBeVisible();
  return pane;
}

async function send(pane: Locator, text: string): Promise<void> {
  const turns = await pane.getByTestId('assistant-turn').count();
  await pane.getByTestId('assistant-input').fill(text);
  await pane.getByTestId('assistant-input').press('Enter');
  await expect(pane.getByTestId('assistant-turn')).toHaveCount(turns + 1);
  await expect(pane.getByTestId('assistant-send')).toBeVisible({ timeout: 15_000 });
}

test.describe('WP-4b screenshots', () => {
  test.skip(!process.env['WP4B_SHOTS'], 'set WP4B_SHOTS=1');
  test.setTimeout(180_000);

  test.afterAll(() => {
    const groups = [...new Set(shots.map((s) => s.group))];
    const body = groups
      .map((g) => `<h2>${g}</h2><div class="grid">${shots.filter((s) => s.group === g).map((s) => `<figure><a href="${s.file}"><img src="${s.file}" alt="${s.label}"></a><figcaption>${s.label}</figcaption></figure>`).join('')}</div>`)
      .join('\n');
    writeFileSync(
      `${OUT}/index.html`,
      `<!doctype html><meta charset="utf-8"><title>Pitwall chat composer</title><style>body{font:13px system-ui;margin:24px;background:#f4f4f6;color:#1d1d1f}h1{font-size:18px}h2{font-size:15px;margin-top:32px}.grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(520px,1fr));gap:16px}figure{margin:0;background:#fff;border:1px solid #ddd;border-radius:8px;padding:8px}img{width:100%;height:auto;display:block}figcaption{margin-top:6px;color:#555}</style><h1>Pitwall 1.1, ADR 0023 WP-4b: the chat composer</h1><p>${shots.length} pictures, browser build with the mock backend, deviceScaleFactor 2.</p>${body}`,
    );
  });

  for (const scheme of ['light', 'dark'] as const) {
    test.describe(scheme, () => {
      test.use({ colorScheme: scheme });

      for (const size of [
        { width: 1440, height: 900 },
        { width: 900, height: 700 },
      ]) {
        test(`files attached, sent and refused at ${size.width}`, async ({ page }) => {
          await page.setViewportSize(size);
          const pane = await chat(page);
          await pane.getByTestId('assistant-file-input').setInputFiles([notes, code]);
          await expect(pane.getByTestId('assistant-file')).toHaveCount(2);
          await pane.getByTestId('assistant-input').fill('Why did the backups pause, and is the cleanup script safe to run?');
          await shot(page, '1-attached', 'Files attached to the next question', 'Two files attached, the meter for a new chat');
          await send(pane, 'Why did the backups pause, and is the cleanup script safe to run?');
          await pane.locator('textarea').blur();
          await page.mouse.move(0, 0);
          await shot(page, '2-sent', 'A sent question with its files', 'The question shows its files; the meter is the engine’s count');
          await pane.getByTestId('assistant-file-input').setInputFiles(photo);
          await expect(pane.getByTestId('assistant-failure')).toBeVisible();
          await shot(page, '3-refused', 'A refusal', 'An image is refused in words (images arrive with WP-5)');
        });
      }

      for (const size of [
        { width: 1440, height: 900 },
        { width: 900, height: 700 },
      ]) {
        test(`the context meter at each level at ${size.width}`, async ({ page }) => {
          await page.setViewportSize(size);
          let pane = await chat(page, `${Q}&mock-context=unknown`);
          await expect(pane.getByTestId('assistant-context')).toHaveAttribute('data-level', 'unknown');
          await shot(page, '4a-meter-unknown', 'The context meter', 'A model whose window is not known: the count alone');
          pane = await chat(page, `${Q}&mock-context=1112`);
          await send(pane, 'How busy is this computer?');
          await expect(pane.getByTestId('assistant-context')).toHaveAttribute('data-level', 'high');
          await shot(page, '4b-meter-high', 'The context meter', 'Close to the limit: words, a glyph and amber');
          pane = await chat(page, `${Q}&mock-context=1094`);
          await send(pane, 'How busy is this computer?');
          await expect(pane.getByTestId('assistant-context-full')).toBeVisible();
          await pane.getByTestId('assistant-input').fill('And the disks?');
          await shot(page, '5-meter-full', 'The context meter', 'Full: the note, Start a new chat, Send off');
        });

        test(`attaching, the refusals and push-to-talk at ${size.width}`, async ({ page }) => {
          await page.setViewportSize(size);
          let pane = await chat(page, `${Q}&mock-speech`);
          await page.evaluate(() => (globalThis as unknown as { __pitwallMock: { holdAttachments(): void } }).__pitwallMock.holdAttachments());
          await pane.getByTestId('assistant-file-input').setInputFiles(notes);
          await expect(pane.getByTestId('assistant-attaching')).toBeVisible();
          await pane.getByTestId('assistant-input').fill('What happened last night?');
          await shot(page, '6a-attaching', 'Attaching', 'A file on its way to Pitwall: Send waits');
          await page.evaluate(() => (globalThis as unknown as { __pitwallMock: { releaseAttachments(): void } }).__pitwallMock.releaseAttachments());
          await expect(pane.getByTestId('assistant-file')).toHaveCount(1);

          const many = Array.from({ length: 16 }, (_, i) => ({ name: `part-${i + 1}.txt`, mimeType: 'text/plain', buffer: Buffer.from(`part ${i + 1}\n`) }));
          await pane.getByTestId('assistant-file-input').setInputFiles(many);
          await expect(pane.getByTestId('assistant-failure')).toContainText('at most 16 attachments');
          await page.mouse.move(0, 0);
          await shot(page, '6b-too-many', 'Refusals', 'Too many files: the sixteenth is the last (under 760 px tall the tray folds to two rows)');
          if (size.height <= 760) {
            await pane.getByTestId('assistant-failure-dismiss').click();
            await pane.getByTestId('assistant-files-summary').click();
            await expect(pane.getByTestId('assistant-files-summary')).toHaveAttribute('aria-expanded', 'true');
            await page.mouse.move(0, 0);
            await shot(page, '6b2-files-open', 'Refusals', 'The folded tray opened from its "16 files" chip');
          }
          pane = await chat(page, `${Q}&mock-speech`);
          await pane.getByTestId('assistant-file-input').setInputFiles({ name: 'huge.log', mimeType: 'text/plain', buffer: Buffer.alloc(300 * 1024, 120) });
          await expect(pane.getByTestId('assistant-failure')).toContainText('larger than 256 KiB');
          await shot(page, '6c-too-big', 'Refusals', 'A file over 256 KiB, refused before it is read');

          pane = await chat(page, `${Q}&mock-speech`);
          await pane.getByTestId('assistant-input').fill('Check the disks:');
          await pane.getByTestId('assistant-mic').click();
          await page.evaluate(() => (globalThis as unknown as { __pitwallSpeech: { tick(ms: number): void } }).__pitwallSpeech.tick(7_400));
          await expect(pane.getByTestId('assistant-recording')).toHaveText('Recording 0:07');
          await page.mouse.move(0, 0);
          await shot(page, '7a-recording', 'Push-to-talk (browser build; the app gets it with WP-7)', 'Recording, with the engine’s time; Send off');
          await page.evaluate(() => (globalThis as unknown as { __pitwallSpeech: { hold(): void } }).__pitwallSpeech.hold());
          await pane.getByTestId('assistant-mic').click();
          await page.mouse.move(0, 0);
          await expect(pane.getByTestId('assistant-transcribing')).toBeVisible();
          await shot(page, '7b-transcribing', 'Push-to-talk (browser build; the app gets it with WP-7)', 'Writing down what was said');
          await page.evaluate(() => (globalThis as unknown as { __pitwallSpeech: { release(): void } }).__pitwallSpeech.release());
          await expect(pane.getByTestId('assistant-input')).toHaveValue(/Check the disks: How busy/);
          await pane.getByTestId('assistant-input').focus();
          await shot(page, '7c-transcribed', 'Push-to-talk (browser build; the app gets it with WP-7)', 'The transcript after the typed text, to edit before sending');
          await page.evaluate(() =>
            (globalThis as unknown as { __pitwallSpeech: { fail(m: string): void } }).__pitwallSpeech.fail(
              'Pitwall may not use the microphone. Allow it in System Settings > Privacy & Security > Microphone, then try again.',
            ),
          );
          await pane.getByTestId('assistant-mic').click();
          await page.mouse.move(0, 0);
          await expect(pane.getByTestId('assistant-failure')).toContainText('Microphone');
          await shot(page, '7d-speech-failed', 'Push-to-talk (browser build; the app gets it with WP-7)', 'Speech input refused by the OS, in words');

          await page.locator('[data-testid=assistant-composer]').evaluate((form) => {
            const data = new DataTransfer();
            data.items.add(new File(['x'], 'log.txt', { type: 'text/plain' }));
            form.dispatchEvent(new DragEvent('dragover', { bubbles: true, cancelable: true, dataTransfer: data }));
          });
          await expect(pane.getByTestId('assistant-composer')).toHaveClass(/dropping/);
          await expect(pane.locator('.drop-hint')).toBeVisible();
          // Let the failure banner's removal and the highlight settle: two frames after the class is set (review 2 P2-E).
          await page.evaluate(() => new Promise<void>((done) => requestAnimationFrame(() => requestAnimationFrame(() => done()))));
          await shot(page, '8-dropping', 'Dropping files', 'Files dragged over the composer');
        });
      }
    });
  }
});
