import { readFileSync } from 'node:fs';
import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';

// About, Updates and What's New in the shell demo (plan S11). The browser build plays the engine with a fake update server
// (`?updates=release|bad-signature|unavailable|...`: no network); one test runs the real Tauri backends against a fake
// `__TAURI_INTERNALS__` that answers as `paddock-updater` does, events included. The version shown is read from its single
// source here too (rule 6), never written into the test.

/** The demo's version: the shell's package version (what `demo-about.ts` imports). */
const VERSION = (JSON.parse(readFileSync('projects/paddock-ui/package.json', 'utf8')) as { version: string }).version;
const dialogUrl = (id: string, search = '') => `/${search}#/dialog?id=${id}&host=main&label=d-main-1&title=${encodeURIComponent(id)}&dialogInput=%7B%7D`;
const toasts = (page: Page) => page.getByRole('region', { name: 'Notifications', exact: true });

test('the version has a release-number shape', () => {
  expect(VERSION).toMatch(/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/);
});

test('About shows the engine’s version and fetches a legal text only when asked', async ({ page }) => {
  await page.goto(dialogUrl('pd.about'));
  await expect(page.getByTestId('about-version')).toHaveText(`Version ${VERSION}`);
  await expect(page.getByTestId('about-legal')).toHaveCount(0);
  await page.getByRole('button', { name: 'Third-party notices' }).click();
  await expect(page.getByTestId('about-legal')).toContainText('Third-party notices (sample)');
  await expect(page.getByRole('button', { name: 'Third-party notices' })).toHaveAttribute('aria-pressed', 'true');
  const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag22aa']).analyze();
  expect(results.violations.map((v) => v.id)).toEqual([]);
});

test('What’s New shows the running version’s notes', async ({ page }) => {
  await page.goto(dialogUrl('pd.whats-new'));
  await expect(page.getByTestId('whats-new-heading')).toHaveText(`What’s New in ${VERSION}`);
  await expect(page.getByRole('region', { name: 'Fixed' })).toContainText('running version’s notes only');
});

test('Help ▸ Check for Updates… finds, downloads and verifies an update, and the main window announces it', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  await page.goto('/');
  await expect(page.getByTestId('app-context')).toHaveText('App context: loaded');
  await page.getByRole('menubar', { name: 'Application menu' }).getByRole('menuitem', { name: 'Help', exact: true }).click();
  const help = page.getByRole('menu', { name: 'Help' });
  await expect(help.getByRole('menuitem', { name: 'What’s New' })).toBeVisible();
  await expect(help.getByRole('menuitem', { name: 'About', exact: true })).toBeVisible();
  await help.getByRole('menuitem', { name: 'Check for Updates…' }).click();
  const dialog = page.getByRole('dialog', { name: 'Updates' });
  await expect(dialog.getByTestId('updates-current')).toHaveText(`You have version ${VERSION}.`);
  await expect(dialog).toHaveAttribute('aria-modal', 'false');
  await dialog.getByRole('button', { name: 'Check now' }).click();
  await expect(dialog.getByTestId('updates-phase')).toContainText('Version 99.0.0 is available (41 MB).');
  await expect(toasts(page)).toContainText('Version 99.0.0 is available');
  await dialog.getByRole('button', { name: 'Download' }).click();
  await expect(dialog.getByTestId('updates-phase')).toContainText('downloaded and its signature verified');
  await expect(dialog.getByRole('button', { name: 'Restart to update' })).toBeVisible();
  await dialog.getByRole('checkbox', { name: 'Install when the app quits' }).check();
  await dialog.getByRole('button', { name: 'Discard' }).click();
  await expect(dialog.getByRole('button', { name: 'Download' })).toBeVisible();
  expect(errors).toEqual([]);
});

test('a package with a bad signature is refused, with the next step', async ({ page }) => {
  await page.goto(dialogUrl('pd.updates', '?updates=bad-signature'));
  await page.getByRole('button', { name: 'Check now' }).click();
  await page.getByRole('button', { name: 'Download' }).click();
  const alert = page.getByTestId('updates-error');
  await expect(alert).toHaveAttribute('role', 'alert');
  await expect(alert).toContainText("The update's signature is not valid, so the update was refused.");
  await expect(alert).toContainText('Do not install it by hand');
  await expect(page.getByRole('button', { name: 'Download' })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Check again' })).toBeVisible();
});

test('a build without an update key says updates are not available, with nothing to press but Close', async ({ page }) => {
  await page.goto(dialogUrl('pd.updates', '?updates=unavailable'));
  await expect(page.getByTestId('updates-phase')).toHaveText('Updates are not available in this build.');
  await expect(page.getByText('without an update signing key')).toBeVisible();
  await expect(page.locator('pd-updates-dialog').getByRole('button')).toHaveCount(0);
  const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag22aa']).analyze();
  expect(results.violations.map((v) => v.id)).toEqual([]);
});

/**
 * A fake Tauri IPC playing the engine with a fake update server: `updates_*` answer as `paddock-updater` does and the
 * result of each job arrives as a `paddock://updates` event; the start-up contract's calls are answered too.
 */
async function fakeEngine(page: Page, version: string): Promise<void> {
  await page.addInitScript((version) => {
    type Listener = { event: string; handler: number };
    const w = window as unknown as Record<string, unknown>;
    const listeners: Listener[] = [];
    const calls: string[] = [];
    let cb = 1;
    const info = { version: '99.0.0', currentVersion: version, notes: 'From the fake server.', pubDate: null, size: 1_000 };
    let status: Record<string, unknown> = {
      currentVersion: version,
      phase: { kind: 'idle' },
      lastChecked: null,
      autoCheck: false,
      channel: 'stable',
      channels: [{ id: 'stable', label: 'Stable' }],
      endpointHost: 'updates.test',
      releasesPage: null,
    };
    const emit = (phase: unknown) => {
      status = { ...status, phase };
      for (const l of listeners.filter((x) => x.event === 'paddock://updates')) {
        (w[`_${l.handler}`] as (e: unknown) => void)({ event: l.event, id: 1, payload: status });
      }
    };
    const answer = (cmd: string, args: Record<string, unknown>): unknown => {
      calls.push(cmd);
      switch (cmd) {
        case 'plugin:event|listen':
          listeners.push({ event: String(args['event']), handler: Number(args['handler']) });
          return listeners.length;
        case 'window_ready':
          return { recovered: null, ui: null };
        case 'updates_status':
          return status;
        case 'updates_check':
          emit({ kind: 'checking', job: 7 });
          // The job's result, later, as an event (the command has returned already).
          queueMicrotask(() => emit({ kind: 'available', info }));
          return status;
        case 'updates_download':
          emit({ kind: 'downloading', info, job: 8 });
          queueMicrotask(() =>
            emit({
              kind: 'failed',
              info,
              error: {
                kind: 'signatureInvalid',
                message: "The update's signature is not valid, so the update was refused.",
                nextStep: 'The download was deleted. Do not install it by hand; check for updates again, and report this to the maintainers if it repeats.',
                detail: 'the signature was made for version 9.9.8, but the release information announces 99.0.0',
              },
            }),
          );
          return status;
        default:
          return null;
      }
    };
    w['__ipcCalls'] = calls;
    w['__TAURI_INTERNALS__'] = {
      transformCallback: (f: unknown) => {
        const id = cb++;
        w[`_${id}`] = f;
        return id;
      },
      invoke: (cmd: string, args: Record<string, unknown>) => Promise.resolve(answer(cmd, args ?? {})),
    };
  }, version);
}

test('over Tauri: the Updates window follows the engine’s events and shows its refusal', async ({ page }) => {
  await fakeEngine(page, VERSION);
  await page.goto(dialogUrl('pd.updates'));
  await expect(page.getByTestId('updates-current')).toHaveText(`You have version ${VERSION}.`);
  await page.getByRole('button', { name: 'Check now' }).click();
  await expect(page.getByTestId('updates-phase')).toContainText('Version 99.0.0 is available (1 KB).');
  await page.getByRole('button', { name: 'Download' }).click();
  await expect(page.getByTestId('updates-error')).toContainText('made for version 9.9.8');
  const calls = await page.evaluate(() => (window as unknown as { __ipcCalls: string[] }).__ipcCalls);
  expect(calls).toEqual(expect.arrayContaining(['window_ready', 'updates_status', 'updates_check', 'updates_download']));
  expect(calls.filter((c) => c.startsWith('updates_') || c.startsWith('about_')).every((c) => ['updates_status', 'updates_check', 'updates_download'].includes(c))).toBe(true);
});
