import { expect, test, type Locator, type Page } from '@playwright/test';
import { mkdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { gradientPng } from '../png.ts';

// The Chat tab's pictures of ADR 0023 WP-5, for the independent review (rule 22): the bar's image line, a picture attached, a
// picture waiting for a model that cannot read it, "Attaching…", the sent question, and the refusals (no vision model, too large,
// a type the chat cannot read, too many); 1440x900 and 900x700, light and dark, @2x; each as the whole window and the pane's
// composer up close.
// Only on request: `WP5_SHOTS=1 npx playwright test -c e2e/screenshots/playwright.config.ts e2e/screenshots/wp5.shot.ts`
// (`WP5_OUT` names another folder). The sheet is index.html next to the pictures (.angular/ is ignored by git).
const OUT = process.env['WP5_OUT'] ? resolve(process.env['WP5_OUT']) : resolve(__dirname, '../../.angular/wp5-shots');
const Q = 'mock-fast&mock-static';

const cat = { name: 'rack-photo.png', mimeType: 'image/png', buffer: gradientPng(96, 64) };
const wide = { name: 'dashboard-screenshot.png', mimeType: 'image/png', buffer: gradientPng(160, 90) };
const tall = { name: 'phone-photo.png', mimeType: 'image/png', buffer: gradientPng(60, 100) };
const huge = { name: 'scan-300dpi.png', mimeType: 'image/png', buffer: Buffer.alloc(26 * 1024 * 1024, 7) };
const tiff = { name: 'scan.tiff', mimeType: 'image/tiff', buffer: Buffer.from([0x49, 0x49, 0x2a, 0x00, 8, 0, 0, 0]) };

const shots: { file: string; group: string; label: string }[] = [];

async function shot(page: Page, name: string, group: string, label: string): Promise<void> {
  mkdirSync(OUT, { recursive: true });
  const scheme = await page.evaluate(() => (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'));
  const width = page.viewportSize()?.width ?? 0;
  const file = `${name}-${width}-${scheme}.png`;
  await page.screenshot({ path: `${OUT}/${file}`, animations: 'disabled' });
  shots.push({ file, group, label: `${label} (${width} px, ${scheme})` });
  const pane = page.locator('pd-assistant');
  const box = await pane.boundingBox();
  if (box) {
    const height = Math.min(box.height, 300);
    const close = `${name}-${width}-${scheme}-composer.png`;
    await page.screenshot({ path: `${OUT}/${close}`, animations: 'disabled', clip: { x: box.x, y: box.y + box.height - height, width: box.width, height } });
    shots.push({ file: close, group, label: `${label}, composer up close (${width} px, ${scheme})` });
  }
}

async function chat(page: Page): Promise<Locator> {
  await page.goto(`/?${Q}#/playground`);
  const pane = page.locator('pw-playground-chat');
  await expect(pane.getByTestId('assistant-empty')).toBeVisible();
  await expect(pane.getByTestId('chat-vision')).toBeVisible();
  return pane;
}

const mock = (page: Page, call: string) => page.evaluate((c) => (globalThis as unknown as { __pitwallMock: Record<string, () => void> }).__pitwallMock[c]!(), call);

test.describe('WP-5 screenshots', () => {
  test.skip(!process.env['WP5_SHOTS'], 'set WP5_SHOTS=1');
  test.setTimeout(180_000);

  test.afterAll(() => {
    const groups = [...new Set(shots.map((s) => s.group))];
    const body = groups
      .map((g) => `<h2>${g}</h2><div class="grid">${shots.filter((s) => s.group === g).map((s) => `<figure><a href="${s.file}"><img src="${s.file}" alt="${s.label}"></a><figcaption>${s.label}</figcaption></figure>`).join('')}</div>`)
      .join('\n');
    writeFileSync(
      `${OUT}/index.html`,
      `<!doctype html><meta charset="utf-8"><title>Pitwall chat pictures</title><style>body{font:13px system-ui;margin:24px;background:#f4f4f6;color:#1d1d1f}h1{font-size:18px}h2{font-size:15px;margin-top:32px}.grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(520px,1fr));gap:16px}figure{margin:0;background:#fff;border:1px solid #ddd;border-radius:8px;padding:8px}img{width:100%;height:auto;display:block}figcaption{margin-top:6px;color:#555}</style><h1>Pitwall 1.1, ADR 0023 WP-5: pictures in the chat</h1><p>${shots.length} pictures, browser build with the mock backend, deviceScaleFactor 2.</p>${body}`,
    );
  });

  for (const scheme of ['light', 'dark'] as const) {
    test.describe(scheme, () => {
      test.use({ colorScheme: scheme });

      for (const size of [
        { width: 1440, height: 900 },
        { width: 900, height: 700 },
      ]) {
        test(`pictures attached, blocked and sent at ${size.width}`, async ({ page }) => {
          await page.setViewportSize(size);
          const pane = await chat(page);
          await page.mouse.move(0, 0);
          await shot(page, '1-no-vision', 'The model cannot read pictures', 'The bar says so and offers the downloaded vision model');
          await pane.getByTestId('assistant-file-input').setInputFiles(cat);
          await expect(pane.getByTestId('assistant-failure')).toContainText("can't read images");
          await page.mouse.move(0, 0);
          await shot(page, '2-refused', 'The model cannot read pictures', 'A picture is refused in words, naming the model to choose');

          await pane.getByTestId('assistant-failure-dismiss').click();
          await pane.getByTestId('chat-use-vision').click();
          await expect(pane.getByTestId('chat-vision')).toHaveText('Reads images.');
          await mock(page, 'holdAttachments');
          await pane.getByTestId('assistant-file-input').setInputFiles(cat);
          await expect(pane.getByTestId('assistant-attaching')).toBeVisible();
          await pane.getByTestId('assistant-input').fill('What is in this picture?');
          await page.mouse.move(0, 0);
          await shot(page, '3-attaching', 'Attaching', 'A picture on its way to Pitwall: Send waits');
          await mock(page, 'releaseAttachments');
          await expect(pane.getByTestId('assistant-file-thumb')).toHaveCount(1);
          await pane.getByTestId('assistant-file-input').setInputFiles([wide, tall]);
          await expect(pane.getByTestId('assistant-file-thumb')).toHaveCount(3);
          await page.mouse.move(0, 0);
          await shot(page, '4-attached', 'Pictures attached', 'Three pictures with thumbnails, size in pixels, and a remove button each');

          await pane.getByTestId('chat-model').click();
          await page.getByRole('option', { name: /Qwen3\.5 9B/ }).click();
          await expect(pane.getByTestId('assistant-image-block')).toBeVisible();
          await expect(pane.getByTestId('assistant-send')).toBeDisabled();
          await page.mouse.move(0, 0);
          await shot(page, '5-blocked', 'A picture waits for a model that cannot read it', 'Send is off; the words say what to choose');

          await pane.getByTestId('chat-use-vision').click();
          await expect(pane.getByTestId('assistant-image-block')).toHaveCount(0);
          await pane.getByTestId('assistant-input').press('Enter');
          await expect(pane.getByTestId('assistant-turn-thumb')).toHaveCount(3);
          await expect(pane.getByTestId('assistant-send')).toBeVisible({ timeout: 15_000 });
          await pane.locator('textarea').blur();
          await page.mouse.move(0, 0);
          await shot(page, '6-sent', 'The sent question', 'The question shows its pictures');
        });

        test(`refusals at ${size.width}`, async ({ page }) => {
          await page.setViewportSize(size);
          const pane = await chat(page);
          await pane.getByTestId('chat-use-vision').click();
          await expect(pane.getByTestId('chat-vision')).toHaveText('Reads images.');
          await pane.getByTestId('assistant-file-input').setInputFiles(huge);
          await expect(pane.getByTestId('assistant-failure')).toContainText('larger than 25 MiB');
          await page.mouse.move(0, 0);
          await shot(page, '7-too-large', 'Refusals', 'A picture over 25 MiB, refused before it is read');
          await pane.getByTestId('assistant-failure-dismiss').click();
          await pane.getByTestId('assistant-file-input').setInputFiles(tiff);
          await expect(pane.getByTestId('assistant-failure')).toContainText("can't read");
          await page.mouse.move(0, 0);
          await shot(page, '8-wrong-type', 'Refusals', 'A picture type the chat cannot read (TIFF), with what to do');
          await pane.getByTestId('assistant-failure-dismiss').click();
          const many = Array.from({ length: 16 }, (_, i) => ({ name: `shot-${i + 1}.png`, mimeType: 'image/png', buffer: gradientPng(40 + i, 30) }));
          await pane.getByTestId('assistant-file-input').setInputFiles(many);
          await expect(pane.getByTestId('assistant-file')).toHaveCount(16);
          await pane.getByTestId('assistant-file-input').setInputFiles(cat);
          await expect(pane.getByTestId('assistant-failure')).toContainText('at most 16 attachments');
          await page.mouse.move(0, 0);
          await shot(page, '9-many', 'Refusals', 'Sixteen pictures in the tray; a seventeenth is refused');
        });
      }
    });
  }
});
