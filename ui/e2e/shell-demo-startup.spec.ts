import { expect, type Page, test } from '@playwright/test';
// The pure splash check, straight from the source: Playwright runs in Node.
import { splashProblems } from '../projects/paddock-ui/shell/startup/src/splash';

// The start-up contract in the shell demo (ADR 0058; plan S7 and §11). A window shows a splash until Angular renders,
// paints its frame without waiting for any IPC, reports `window_ready` after that first paint, says calmly when the shell
// reloaded it and gets its UI state back, and answers Rust's liveness pings once ready. The IPC tests run the real Tauri
// backend against a fake `__TAURI_INTERNALS__` whose replies the test holds and releases (a paused web content process,
// a lost first IPC message) and whose ping events the test sends (Rust's ticker).

interface ReadyReply {
  recovered: 'unresponsive' | 'hung' | 'terminated' | null;
  ui: unknown;
}

/** Installs a fake Tauri IPC that records every call and holds every reply until `__ipcRelease()` (or at once if `open`). */
async function fakeTauri(page: Page, reply: ReadyReply, open = false): Promise<void> {
  await page.addInitScript(
    ({ reply, open }) => {
      type Held = { cmd: string; args: unknown; resolve: (v: unknown) => void; reject: (e: unknown) => void };
      const w = window as unknown as Record<string, unknown>;
      const held: Held[] = [];
      const calls: { cmd: string; args: unknown; at: number }[] = [];
      let released = open;
      let cb = 1;
      let eventId = 1;
      const answer = (h: Held) => {
        if (h.cmd === 'window_ready') h.resolve(reply);
        else if (h.cmd === 'plugin:event|listen') h.resolve(eventId++);
        else if (h.cmd === 'window_pong' || h.cmd === 'ui_checkpoint' || h.cmd === 'window_boot_error') h.resolve(null);
        else h.reject('unavailable in this test');
      };
      w['__ipcCalls'] = calls;
      w['__ipcRelease'] = () => {
        released = true;
        held.splice(0).forEach(answer);
      };
      /** Rust's ticker: sends ping `seq` to every `paddock://ping` listener. */
      w['__ping'] = (seq: number) => {
        for (const c of calls) {
          const a = c.args as { event?: string; handler?: number };
          if (c.cmd === 'plugin:event|listen' && a.event === 'paddock://ping') {
            (w[`_${a.handler}`] as (e: unknown) => void)({ event: 'paddock://ping', id: 1, payload: { seq } });
          }
        }
      };
      w['__TAURI_INTERNALS__'] = {
        transformCallback: (f: unknown) => {
          const id = cb++;
          w[`_${id}`] = f;
          return id;
        },
        invoke: (cmd: string, args: unknown) =>
          new Promise((resolve, reject) => {
            calls.push({ cmd, args, at: Date.now() });
            const h = { cmd, args, resolve, reject };
            if (released) answer(h);
            else held.push(h);
          }),
      };
    },
    { reply, open },
  );
}

const calls = (page: Page) => page.evaluate(() => (window as unknown as { __ipcCalls: { cmd: string; args: unknown; at: number }[] }).__ipcCalls.slice());
const names = async (page: Page) => (await calls(page)).map((c) => c.cmd);
const release = (page: Page) => page.evaluate(() => (window as unknown as { __ipcRelease: () => void }).__ipcRelease());
const ping = (page: Page, seq: number) => page.evaluate((s) => (window as unknown as { __ping: (s: number) => void }).__ping(s), seq);
/** The event names the window listens for (`plugin:event|listen`); the appearance listener (plan S15) is not a ping one. */
const listens = async (page: Page) =>
  (await calls(page)).filter((c) => c.cmd === 'plugin:event|listen').map((c) => (c.args as { event?: string }).event ?? '');
const pongs = async (page: Page) =>
  (await calls(page)).filter((c) => c.cmd === 'window_pong').map((c) => ({ seq: (c.args as { seq: number }).seq, at: c.at }));

const heading = (page: Page) => page.getByRole('heading', { level: 1, name: 'Paddock shell demo' });
// Notices are the notification host's toasts (plan S4).
const notices = (page: Page) => page.getByRole('region', { name: 'Notifications', exact: true });
const note = (page: Page) => page.getByRole('textbox', { name: 'Note' });

test.describe('start-up contract (plan S7)', () => {
  test('the window shows a splash until Angular renders, and the page passes the splash check', async ({ page, request }) => {
    expect(splashProblems(await (await request.get('/')).text(), 'sd-root')).toEqual([]);
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

  test('with every IPC reply withheld (a paused web content process) the frame still paints and reports ready', async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (e) => errors.push(e.message));
    await fakeTauri(page, { recovered: null, ui: null });
    await page.goto('/');
    // Nothing has been answered, yet the frame is there.
    await expect(heading(page)).toBeVisible();
    await expect(page.getByRole('group', { name: 'Commands' }).getByRole('button', { name: 'Say hello' })).toBeVisible();
    await expect(note(page)).toBeVisible();
    // Ready is reported after the first paint, without waiting for any other reply; no ping listener before the reply.
    await expect.poll(() => names(page)).toContain('window_ready');
    expect(await listens(page)).not.toContain('paddock://ping');
    // The process resumes: the replies arrive; the window stays usable and listens for pings.
    await release(page);
    await expect.poll(() => listens(page)).toContain('paddock://ping');
    await page.getByRole('button', { name: 'Say hello' }).click();
    await expect(notices(page).getByText('Hello from the shell demo (1)')).toBeVisible();
    expect((await names(page)).filter((c) => c === 'window_ready')).toHaveLength(1);
    expect(await notices(page).getByText('was reloaded').count()).toBe(0);
    expect(errors).toEqual([]);
  });

  for (const [why, text] of [
    ['terminated', "The window's content stopped unexpectedly and was reloaded. Your open files and view are kept."],
    ['unresponsive', 'The window did not finish starting and was reloaded. Your open files and view are kept.'],
    ['hung', 'The window stopped responding and was reloaded. Your open files and view are kept.'],
  ] as const) {
    test(`a window the shell reloaded (${why}) says so calmly and gets its note back`, async ({ page }) => {
      await fakeTauri(page, { recovered: why, ui: { note: `kept after ${why}` } }, true);
      await page.goto('/');
      await expect(notices(page).getByText(text)).toBeVisible();
      await expect(note(page)).toHaveValue(`kept after ${why}`);
    });
  }

  test('the note is checkpointed to Rust after a quiet second, once the ready reply was handled', async ({ page }) => {
    await fakeTauri(page, { recovered: null, ui: null }, true);
    await page.goto('/');
    await expect.poll(() => listens(page)).toContain('paddock://ping');
    await note(page).fill('order 42');
    await expect
      .poll(async () => (await calls(page)).filter((c) => c.cmd === 'ui_checkpoint').map((c) => c.args))
      .toContainEqual({ ui: { note: 'order 42' } });
  });

  test('a page answers each ping once ready, and a page that stops answering answers nothing until it runs again', async ({ page }) => {
    await fakeTauri(page, { recovered: null, ui: null }, true);
    await page.goto('/');
    await expect.poll(() => listens(page)).toContain('paddock://ping');
    await ping(page, 1);
    await ping(page, 2);
    await expect.poll(async () => (await pongs(page)).map((p) => p.seq)).toEqual([1, 2]);
    // The page's thread stops (a hang after ready) for 1.5 s and a ping arrives meanwhile (queued behind the hang, as an
    // event from Rust is): it is answered only when the page runs again, so Rust's watchdog counts the misses and offers
    // Reload / Keep Waiting.
    const { frozeAt } = await page.evaluate(() => {
      const w = window as unknown as { __ping: (s: number) => void; __thawedAt?: number };
      setTimeout(() => {
        const end = performance.now() + 1500;
        while (performance.now() < end) {
          // busy: the page does not answer
        }
        w.__thawedAt = Date.now();
      }, 0);
      setTimeout(() => w.__ping(3), 0);
      return { frozeAt: Date.now() };
    });
    await expect.poll(async () => (await pongs(page)).map((p) => p.seq)).toEqual([1, 2, 3]);
    const thawedAt = await page.evaluate(() => (window as unknown as { __thawedAt: number }).__thawedAt);
    const third = (await pongs(page))[2]!;
    expect(third.at).toBeGreaterThanOrEqual(thawedAt);
    expect(third.at - frozeAt).toBeGreaterThanOrEqual(1400);
  });

  test('in the browser build a recovery reload brings the note back, and a reload the user asks for starts clean', async ({ page }) => {
    await page.goto('/');
    await note(page).fill('draft');
    await expect.poll(() => page.evaluate(() => sessionStorage.getItem('paddock.mock.uiCheckpoint') ?? '')).toContain('"note":"draft"');
    // The shell reloads the window after it stopped answering.
    await page.evaluate(() => sessionStorage.setItem('paddock.mock.recover', 'hung'));
    await page.reload();
    await expect(notices(page).getByText('The window stopped responding and was reloaded. Your open files and view are kept.')).toBeVisible();
    await expect(note(page)).toHaveValue('draft');
    await page.reload();
    await expect(heading(page)).toBeVisible();
    await expect(note(page)).toHaveValue('');
  });
});
