import { expect, installTauriFake, test, type Page, type TauriFake } from '@decryp7/paddock-e2e';

// The start-up contract in the shell demo (ADR 0058; plan S7 and §11). A window shows a splash until Angular renders,
// paints its frame without waiting for any IPC, reports `window_ready` after that first paint, says calmly when the shell
// reloaded it and gets its UI state back, and answers Rust's liveness pings once ready. The IPC tests run the real Tauri
// backend against the kit's fake `__TAURI_INTERNALS__` (@decryp7/paddock-e2e), whose replies the test holds and releases
// (a paused web content process, a lost first IPC message) and whose ping events the test sends (Rust's ticker). The
// splash markup itself is checked in shell/testing's reference-sources spec.

type Recovered = 'unresponsive' | 'hung' | 'terminated' | null;

/** The fake IPC with the ready reply of a window that was (or was not) recovered; `hold` keeps every reply until released. */
const fakeTauri = (page: Page, recovered: Recovered, ui: unknown, hold: boolean): Promise<TauriFake> =>
  installTauriFake(page, { hold, replies: { window_ready: { recovered, ui } } });

/** Rust's ticker: ping `seq` to every `paddock://ping` listener. */
const ping = (ipc: TauriFake, seq: number) => ipc.emit('paddock://ping', { seq });
const pongs = async (ipc: TauriFake) => (await ipc.callsOf('window_pong')).map((c) => ({ seq: (c.args as { seq: number }).seq, at: c.at }));

const heading = (page: Page) => page.getByRole('heading', { level: 1, name: 'Overview' });
const note = (page: Page) => page.getByRole('textbox', { name: 'Note' });

test.describe('start-up contract (plan S7)', () => {
  test('the window shows a splash until Angular renders', async ({ page }) => {
    let releaseScript!: () => void;
    const held = new Promise<void>((r) => (releaseScript = r));
    await page.route(/\/main(-[A-Z0-9]+)?\.js$/, async (route) => {
      await held;
      await route.continue();
    });
    await page.goto('/', { waitUntil: 'commit' });
    await expect(page.getByTestId('boot-splash')).toContainText('Starting shell demo…');
    await expect(page.getByRole('status')).toBeVisible();
    releaseScript();
    await expect(heading(page)).toBeVisible();
    await expect(page.getByTestId('boot-splash')).toHaveCount(0);
  });

  test('with every IPC reply withheld (a paused web content process) the frame still paints and reports ready', async ({ app, page, pageErrors }) => {
    const ipc = await fakeTauri(page, null, null, true);
    await page.goto('/');
    // Nothing has been answered, yet the frame is there.
    await expect(heading(page)).toBeVisible();
    await expect(page.getByRole('group', { name: 'Commands' }).getByRole('button', { name: 'Say hello' })).toBeVisible();
    await expect(note(page)).toBeVisible();
    // Ready is reported after the first paint, without waiting for any other reply; no ping listener before the reply.
    await expect.poll(() => ipc.names()).toContain('window_ready');
    expect(await ipc.names()).not.toContain('plugin:event|listen');
    // The process resumes: the replies arrive; the window stays usable and listens for pings.
    await ipc.release();
    await expect.poll(() => ipc.names()).toContain('plugin:event|listen');
    await page.getByRole('button', { name: 'Say hello' }).click();
    await expect(app.notifications.toasts.getByText('Hello from the shell demo (1)')).toBeVisible();
    expect(await ipc.callsOf('window_ready')).toHaveLength(1);
    expect(await app.notifications.toasts.getByText('was reloaded').count()).toBe(0);
    expect(pageErrors).toEqual([]);
  });

  for (const [why, text] of [
    ['terminated', "The window's content stopped unexpectedly and was reloaded. Your open files and view are kept."],
    ['unresponsive', 'The window did not finish starting and was reloaded. Your open files and view are kept.'],
    ['hung', 'The window stopped responding and was reloaded. Your open files and view are kept.'],
  ] as const) {
    test(`a window the shell reloaded (${why}) says so calmly and gets its note back`, async ({ app, page }) => {
      await fakeTauri(page, why, { note: `kept after ${why}` }, false);
      await page.goto('/');
      await expect(app.notifications.toasts.getByText(text)).toBeVisible();
      await expect(note(page)).toHaveValue(`kept after ${why}`);
    });
  }

  test('the note is checkpointed to Rust after a quiet second, once the ready reply was handled', async ({ page }) => {
    const ipc = await fakeTauri(page, null, null, false);
    await page.goto('/');
    await expect.poll(() => ipc.names()).toContain('plugin:event|listen');
    await note(page).fill('order 42');
    await expect
      .poll(async () => (await ipc.callsOf('ui_checkpoint')).map((c) => c.args))
      .toContainEqual({ ui: { note: 'order 42' } });
  });

  test('a page answers each ping once ready, and a page that stops answering answers nothing until it runs again', async ({ page }) => {
    const ipc = await fakeTauri(page, null, null, false);
    await page.goto('/');
    await expect.poll(() => ipc.names()).toContain('plugin:event|listen');
    await ping(ipc, 1);
    await ping(ipc, 2);
    await expect.poll(async () => (await pongs(ipc)).map((p) => p.seq)).toEqual([1, 2]);
    // The page's thread stops (a hang after ready) for 1.5 s and a ping arrives meanwhile (queued behind the hang, as an
    // event from Rust is): it is answered only when the page runs again, so Rust's watchdog counts the misses and offers
    // Reload / Keep Waiting.
    const { frozeAt } = await page.evaluate(() => {
      const w = window as unknown as { __paddockTauri: { emit: (e: string, p: unknown) => number }; __thawedAt?: number };
      setTimeout(() => {
        const end = performance.now() + 1500;
        while (performance.now() < end) {
          // busy: the page does not answer
        }
        w.__thawedAt = Date.now();
      }, 0);
      setTimeout(() => w.__paddockTauri.emit('paddock://ping', { seq: 3 }), 0);
      return { frozeAt: Date.now() };
    });
    await expect.poll(async () => (await pongs(ipc)).map((p) => p.seq)).toEqual([1, 2, 3]);
    const thawedAt = await page.evaluate(() => (window as unknown as { __thawedAt: number }).__thawedAt);
    const third = (await pongs(ipc))[2]!;
    expect(third.at).toBeGreaterThanOrEqual(thawedAt);
    expect(third.at - frozeAt).toBeGreaterThanOrEqual(1400);
  });

  test('in the browser build a recovery reload brings the note back, and a reload the user asks for starts clean', async ({ app, page }) => {
    await page.goto('/');
    await note(page).fill('draft');
    await expect.poll(() => page.evaluate(() => sessionStorage.getItem('paddock.mock.uiCheckpoint') ?? '')).toContain('"note":"draft"');
    // The shell reloads the window after it stopped answering.
    await page.evaluate(() => sessionStorage.setItem('paddock.mock.recover', 'hung'));
    await page.reload();
    await expect(app.notifications.toasts.getByText('The window stopped responding and was reloaded. Your open files and view are kept.')).toBeVisible();
    await expect(note(page)).toHaveValue('draft');
    await page.reload();
    await expect(heading(page)).toBeVisible();
    await expect(note(page)).toHaveValue('');
  });
});
