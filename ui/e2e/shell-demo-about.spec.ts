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
  await expect(page.getByTestId('whats-new-heading')).toHaveText(`What’s new in ${VERSION}`);
  await expect(page.getByRole('region', { name: 'Fixed' })).toContainText('running version’s notes only');
});

test('Help ▸ Check for updates… finds, downloads and verifies an update, and the main window announces it', async ({ app, page, pageErrors }) => {
  await page.goto('/');
  await expect(page.getByTestId('app-context')).toHaveText('App context: loaded');
  const help = await app.menus.open('Help');
  await expect(help.getByRole('menuitem', { name: 'What’s new' })).toBeVisible();
  await expect(help.getByRole('menuitem', { name: 'About', exact: true })).toBeVisible();
  await help.getByRole('menuitem', { name: 'Check for updates…' }).click();
  const dialog = page.getByRole('dialog', { name: 'Updates' });
  await expect(dialog.getByTestId('updates-current')).toHaveText(`You have version ${VERSION}.`);
  await expect(dialog).toHaveAttribute('aria-modal', 'false');
  await dialog.getByRole('button', { name: 'Check now' }).click();
  await expect(dialog.getByTestId('updates-phase')).toContainText('Version 99.0.0 is available (39 MB).');
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

test('the Updates preferences are Material checkboxes and pass axe in both themes', async ({ app, page }) => {
  for (const colorScheme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme });
    await app.dialogs.openWindow('pd.updates');
    const auto = page.locator('pd-updates-dialog mat-checkbox').first();
    await expect(auto).toContainText('Check for updates automatically');
    await auto.getByRole('checkbox').check();
    await expect(auto.getByRole('checkbox')).toBeChecked();
    expect(await axeViolations(page, { idsOnly: true })).toEqual([]);
  }
});

test('the download shows a determinate Material progress bar, and an indeterminate one without a total', async ({ app, page }) => {
  await page.goto('/?updates=release&hold=1');
  await expect(page.getByTestId('app-context')).toHaveText('App context: loaded');
  await (await app.menus.open('Help')).getByRole('menuitem', { name: 'Check for updates…' }).click();
  const dialog = page.getByRole('dialog', { name: 'Updates' });
  await dialog.getByRole('button', { name: 'Check now' }).click();
  await dialog.getByRole('button', { name: 'Download' }).click();
  await expect(dialog.getByTestId('updates-phase')).toContainText('Downloading version 99.0.0');
  const bar = dialog.getByRole('progressbar');
  await expect(bar).toHaveCount(0);
  await page.evaluate(() => (window as unknown as { sdUpdatesDownload: { progress(d: number, t: number | null): void } }).sdUpdatesDownload.progress(20_500_000, 41_000_000));
  await expect(bar).toHaveAttribute('aria-valuenow', '50');
  await expect(bar).toHaveAccessibleName('Downloading: 20 MB of 39 MB');
  expect(await axeViolations(page, { idsOnly: true })).toEqual([]);
  await page.evaluate(() => (window as unknown as { sdUpdatesDownload: { progress(d: number, t: number | null): void } }).sdUpdatesDownload.progress(1000, null));
  await expect(dialog.locator('mat-progress-bar')).toHaveAttribute('mode', 'indeterminate');
  await expect(bar).not.toHaveAttribute('aria-valuenow', /.+/);
  // With reduced motion it is a full, still bar, not a frozen partial one that reads as a value.
  await page.emulateMedia({ reducedMotion: 'reduce' });
  const widths = await dialog.locator('.mdc-linear-progress__primary-bar').evaluate((e) => [e.getBoundingClientRect().width, e.parentElement!.getBoundingClientRect().width]);
  expect(Math.abs(widths[0]! - widths[1]!)).toBeLessThanOrEqual(1);
  await page.evaluate(() => (window as unknown as { sdUpdatesDownload: { finish(): void } }).sdUpdatesDownload.finish());
  await expect(dialog.getByTestId('updates-phase')).toContainText('downloaded and its signature verified');
});

test('a build without an update key says updates are not available, with nothing to press but Close', async ({ app, page }) => {
  await app.dialogs.openWindow('pd.updates', {}, { query: '?updates=unavailable' });
  await expect(page.getByTestId('updates-phase')).toHaveText('Updates are not available in this build.');
  await expect(page.getByText('without an update signing key')).toBeVisible();
  await expect(page.locator('pd-updates-dialog').getByRole('button')).toHaveCount(0);
  expect(await axeViolations(page, { idsOnly: true })).toEqual([]);
});

/** The engine's updater status (`updates_status`) with `phase`, as `paddock-updater` reports it. */
const PER_USER = { scope: 'perUser', installOnQuitAvailable: true, administratorNotice: null };
const status = (phase: unknown, install: unknown = PER_USER) => ({
  currentVersion: VERSION,
  phase,
  lastChecked: null,
  autoCheck: false,
  channel: 'stable',
  channels: [{ id: 'stable', label: 'Stable' }],
  endpointHost: 'updates.test',
  releasesPage: null,
  install,
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
      // The window's start data comes from the plugin (`dialog_take_input`), as in the desktop app.
      dialog_take_input: { dialogId: 'pd.updates', title: 'Updates', kind: 'tool', modality: 'modeless', requiredChoice: false, input: {} },
    },
  });

test('over Tauri: the Updates window follows the engine’s events and shows its refusal', async ({ app, page }) => {
  const ipc = await fakeEngine(page);
  await app.dialogs.openWindow('pd.updates');
  await expect(page.getByTestId('updates-current')).toHaveText(`You have version ${VERSION}.`);
  await page.getByRole('button', { name: 'Check now' }).click();
  await expect.poll(() => ipc.names()).toContain('updates_check');
  expect(await ipc.emit('paddock://updates', status({ kind: 'available', info: INFO }))).toBeGreaterThan(0);
  await expect(page.getByTestId('updates-phase')).toContainText('Version 99.0.0 is available (1,000 bytes).');
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

test('over Tauri: a per-machine copy shows Install, the administrator text and no install on quit, and a declined prompt offers Install again', async ({ app, page }) => {
  const notice = "An administrator must install this update. Windows will ask for an administrator's name and password.";
  const perMachine = { scope: 'perMachine', installOnQuitAvailable: false, administratorNotice: notice };
  const ipc = await installTauriFake(page, {
    otherwise: 'null',
    replies: {
      updates_status: status({ kind: 'ready', info: INFO, installOnQuit: false }, perMachine),
      dialog_take_input: { dialogId: 'pd.updates', title: 'Updates', kind: 'tool', modality: 'modeless', requiredChoice: false, input: {} },
    },
  });
  await app.dialogs.openWindow('pd.updates');
  await expect(page.getByTestId('updates-phase')).toContainText('downloaded and its signature verified');
  await expect(page.getByTestId('updates-administrator')).toHaveText(notice);
  await expect(page.getByRole('button', { name: 'Install', exact: true })).toBeVisible();
  await expect(page.getByText('Install when the app quits')).toHaveCount(0);
  // The old version starts again after a declined prompt and says so.
  await ipc.emit(
    'paddock://updates',
    status(
      {
        kind: 'failed',
        info: null,
        error: { kind: 'declined', message: 'Not installed: Windows permission was declined.', nextStep: 'Click Install again and choose Yes in the Windows prompt.', detail: null },
      },
      perMachine,
    ),
  );
  await expect(page.getByTestId('updates-error')).toContainText('Windows permission was declined');
  await expect(page.getByRole('button', { name: 'Install again' })).toBeVisible();
  await ipc.emit('paddock://updates', status({ kind: 'installerRunning', version: '99.0.0' }, perMachine));
  await expect(page.getByTestId('updates-installer-running')).toContainText('An update is being installed');
});
