import { axeViolations, expect, installTauriFake, readVersion, test, type Page, type TauriFake } from '@decryp7/paddock-e2e';

// About, Updates and What's New in the shell demo (plan S11). The browser build plays the engine with a fake update server
// (`?updates=release|bad-signature|unavailable|...`: no network); one test runs the real Tauri backends against a fake
// `__TAURI_INTERNALS__` that answers as `paddock-updater` does, events included. The version shown is read from its single
// source here too (rule 6, the kit's `readVersion`, which also checks its release-number shape), never written into the
// test. Only the test kit (@decryp7/paddock-e2e, plan S16).

/** The demo's version: the shell's package version (what `demo-about.ts` imports). */
const VERSION = readVersion('projects/paddock-ui/package.json');

test('About shows the engine’s version and fetches a legal text only when asked', async ({ app, page }) => {
  await app.dialogs.openWindow('pd.about');
  await expect(page.getByTestId('about-version')).toHaveText(`Version ${VERSION}`);
  await expect(page.getByTestId('about-legal')).toHaveCount(0);
  await page.getByRole('button', { name: 'Third-party notices' }).click();
  await expect(page.getByTestId('about-legal')).toContainText('Third-party notices (sample)');
  await expect(page.getByRole('button', { name: 'Third-party notices' })).toHaveAttribute('aria-pressed', 'true');
  expect(await axeViolations(page, { idsOnly: true })).toEqual([]);
});

test('What’s New shows the running version’s notes', async ({ app, page }) => {
  await app.dialogs.openWindow('pd.whats-new');
  await expect(page.getByTestId('whats-new-heading')).toHaveText(`What’s New in ${VERSION}`);
  await expect(page.getByRole('region', { name: 'Fixed' })).toContainText('running version’s notes only');
});

test('Help ▸ Check for Updates… finds, downloads and verifies an update, and the main window announces it', async ({ app, page, pageErrors }) => {
  await page.goto('/');
  await expect(page.getByTestId('app-context')).toHaveText('App context: loaded');
  const help = await app.menus.open('Help');
  await expect(help.getByRole('menuitem', { name: 'What’s New' })).toBeVisible();
  await expect(help.getByRole('menuitem', { name: 'About', exact: true })).toBeVisible();
  await help.getByRole('menuitem', { name: 'Check for Updates…' }).click();
  const dialog = page.getByRole('dialog', { name: 'Updates' });
  await expect(dialog.getByTestId('updates-current')).toHaveText(`You have version ${VERSION}.`);
  await expect(dialog).toHaveAttribute('aria-modal', 'false');
  await dialog.getByRole('button', { name: 'Check now' }).click();
  await expect(dialog.getByTestId('updates-phase')).toContainText('Version 99.0.0 is available (41 MB).');
  await expect(app.notifications.toasts).toContainText('Version 99.0.0 is available');
  await dialog.getByRole('button', { name: 'Download' }).click();
  await expect(dialog.getByTestId('updates-phase')).toContainText('downloaded and its signature verified');
  await expect(dialog.getByRole('button', { name: 'Restart to update' })).toBeVisible();
  await dialog.getByRole('checkbox', { name: 'Install when the app quits' }).check();
  await dialog.getByRole('button', { name: 'Discard' }).click();
  await expect(dialog.getByRole('button', { name: 'Download' })).toBeVisible();
  expect(pageErrors).toEqual([]);
});

test('a package with a bad signature is refused, with the next step', async ({ app, page }) => {
  await app.dialogs.openWindow('pd.updates', {}, { query: '?updates=bad-signature' });
  await page.getByRole('button', { name: 'Check now' }).click();
  await page.getByRole('button', { name: 'Download' }).click();
  const alert = page.getByTestId('updates-error');
  await expect(alert).toHaveAttribute('role', 'alert');
  await expect(alert).toContainText("The update's signature is not valid, so the update was refused.");
  await expect(alert).toContainText('Do not install it by hand');
  await expect(page.getByRole('button', { name: 'Download' })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Check again' })).toBeVisible();
});

test('a build without an update key says updates are not available, with nothing to press but Close', async ({ app, page }) => {
  await app.dialogs.openWindow('pd.updates', {}, { query: '?updates=unavailable' });
  await expect(page.getByTestId('updates-phase')).toHaveText('Updates are not available in this build.');
  await expect(page.getByText('without an update signing key')).toBeVisible();
  await expect(page.locator('pd-updates-dialog').getByRole('button')).toHaveCount(0);
  expect(await axeViolations(page, { idsOnly: true })).toEqual([]);
});

/** The engine's updater status (`updates_status`) with `phase`, as `paddock-updater` reports it. */
const status = (phase: unknown) => ({
  currentVersion: VERSION,
  phase,
  lastChecked: null,
  autoCheck: false,
  channel: 'stable',
  channels: [{ id: 'stable', label: 'Stable' }],
  endpointHost: 'updates.test',
  releasesPage: null,
});
const INFO = { version: '99.0.0', currentVersion: VERSION, notes: 'From the fake server.', pubDate: null, size: 1_000 };

/**
 * The kit's fake Tauri IPC playing the engine with a fake update server: `updates_*` answer as `paddock-updater` does
 * (the job started), and the test sends each job's result as a `paddock://updates` event, as Rust does later.
 */
const fakeEngine = (page: Page): Promise<TauriFake> =>
  installTauriFake(page, {
    otherwise: 'null',
    replies: {
      updates_status: status({ kind: 'idle' }),
      updates_check: status({ kind: 'checking', job: 7 }),
      updates_download: status({ kind: 'downloading', info: INFO, job: 8 }),
    },
  });

test('over Tauri: the Updates window follows the engine’s events and shows its refusal', async ({ app, page }) => {
  const ipc = await fakeEngine(page);
  await app.dialogs.openWindow('pd.updates');
  await expect(page.getByTestId('updates-current')).toHaveText(`You have version ${VERSION}.`);
  await page.getByRole('button', { name: 'Check now' }).click();
  await expect.poll(() => ipc.names()).toContain('updates_check');
  expect(await ipc.emit('paddock://updates', status({ kind: 'available', info: INFO }))).toBeGreaterThan(0);
  await expect(page.getByTestId('updates-phase')).toContainText('Version 99.0.0 is available (1 KB).');
  await page.getByRole('button', { name: 'Download' }).click();
  await expect.poll(() => ipc.names()).toContain('updates_download');
  await ipc.emit(
    'paddock://updates',
    status({
      kind: 'failed',
      info: INFO,
      error: {
        kind: 'signatureInvalid',
        message: "The update's signature is not valid, so the update was refused.",
        nextStep: 'The download was deleted. Do not install it by hand; check for updates again, and report this to the maintainers if it repeats.',
        detail: 'the signature was made for version 9.9.8, but the release information announces 99.0.0',
      },
    }),
  );
  await expect(page.getByTestId('updates-error')).toContainText('made for version 9.9.8');
  const calls = await ipc.names();
  expect(calls).toEqual(expect.arrayContaining(['window_ready', 'updates_status', 'updates_check', 'updates_download']));
  expect(calls.filter((c) => c.startsWith('updates_') || c.startsWith('about_')).every((c) => ['updates_status', 'updates_check', 'updates_download'].includes(c))).toBe(true);
});
