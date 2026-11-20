import { expect, test } from '@playwright/test';
import type { Page } from '@playwright/test';
import { expectStreamDone } from '../src/index.ts';

// `expectStreamDone` waits for a stream (a live region marked `aria-busy` while it is updated) to finish. A stream that keeps arriving is not
// stuck, however long it takes: on a loaded runner Debrief's scripted answer (20 pieces 150 ms apart, 3.4 s on an idle Mac) outlasted the 5 s
// expect timeout while still streaming, in both tries (debrief.yml run 8377 job 42784, ai-chat.e2e.ts:116). A stream that stops arriving fails.

/** The stall window of the stuck-stream test, so it fails fast. */
const WINDOW_MS = 400;

// The timers are the subject here, not waits, and C71 (rule 49) reads the call names: inside the page they are called through the window by name.
type Timers = Record<'setInterval' | 'clearInterval', (...a: unknown[]) => unknown>;

/** A log that is busy and gets one piece every `everyMs` for `pieces` pieces, then `aria-busy="false"`, or stays busy when `ends` is false. */
async function streamingLog(page: Page, everyMs: number, pieces: number, ends = true): Promise<void> {
  await page.setContent('<div role="log" aria-label="Conversation" aria-busy="true"></div>');
  await page.evaluate(
    ([everyMs, pieces, ends]) => {
      const log = document.querySelector('[role=log]')!;
      const timers = window as unknown as Timers;
      let sent = 0;
      const id = timers.setInterval(() => {
        if (sent === pieces) {
          if (ends) log.setAttribute('aria-busy', 'false');
          timers.clearInterval(id);
          return;
        }
        log.append(`piece ${sent++} `);
      }, everyMs);
    },
    [everyMs, pieces, ends] as const,
  );
}

test('a stream that keeps arriving for longer than the expect timeout is waited for until it is done', async ({ page }) => {
  // 65 pieces 100 ms apart: about 6.5 s, past the default expect timeout (5 s) that bounded the whole wait in run 8377.
  await streamingLog(page, 100, 65);
  const log = page.getByRole('log', { name: 'Conversation' });
  await expectStreamDone(log);
  await expect(log).toHaveAttribute('aria-busy', 'false', { timeout: 0 });
  await expect(log).toContainText('piece 64', { timeout: 0 });
});

// The positive controls (ER-33): a stream that stops arriving is stuck, and the wait fails with what it saw, whether it never started or stopped.
test('a stream that stays busy with nothing arriving fails in its first wait, naming the stall', async ({ page }) => {
  await streamingLog(page, 100, 0, false);
  const error = await expectStreamDone(page.getByRole('log', { name: 'Conversation' }), { timeout: WINDOW_MS }).then(
    () => null,
    (e: Error) => e,
  );
  expect(error?.message).toContain('the stream has finished (aria-busy is "false")');
  expect(error?.message).toContain('The stream is stuck: nothing new arrived during wait 1 ');
});

test('a stream that stops arriving while still busy fails in the first wait after it stopped', async ({ page }) => {
  // 8 pieces 100 ms apart arrive over the first two or three windows of 400 ms, then nothing.
  await streamingLog(page, 100, 8, false);
  const error = await expectStreamDone(page.getByRole('log', { name: 'Conversation' }), { timeout: WINDOW_MS }).then(
    () => null,
    (e: Error) => e,
  );
  expect(error?.message).toMatch(/The stream is stuck: nothing new arrived during wait [2-9] .*it shows 64 characters/s);
});
