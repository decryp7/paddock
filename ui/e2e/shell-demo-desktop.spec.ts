import { axeViolations, expect, installTauriFake, readVersion, test, type Page, type TauriFake } from '@decryp7/paddock-e2e';
import { SETTINGS_OVERVIEW_FIXTURE } from '../projects/paddock-ui/shell/settings/generated/settings-fixtures.ts';

// The shell demo as the desktop app (plan S18, apps/shell-demo): every part on its Tauri backend, played by the kit's
// fake `__TAURI_INTERNALS__` (@decryp7/paddock-e2e). The demo's own commands go to its Rust engine (`demo_break`,
// `demo_job_start`), the shell's to the plugin, and Rust's answers come back as events, as the app's tests on Tauri's
// mock runtime check from the other side (apps/shell-demo/src-tauri/tests/app.rs).

/** The shell's version (the plugin answers the UI package's) and the app's own: each from its single source (rule 6). */
const SHELL_VERSION = readVersion('projects/paddock-ui/package.json');
const APP_VERSION = readVersion('../apps/shell-demo/src-tauri/Cargo.toml');
const VERSION_REPLY = { shell: SHELL_VERSION, app: { id: 'shell-demo', name: 'Paddock Shell Demo', version: APP_VERSION } };

const cause = (message: string, hint: string, detail: string, code: string) => ({ message, hint, detail, code, raiseCap: null, settings: null });

const service = (id: string, label: string, state: string, c: ReturnType<typeof cause> | null = null, repair: string | null = null) => ({
  service: id,
  label,
  state,
  cause: c,
  repair,
});

const ENGINE_DOWN = cause('The demo engine stopped unexpectedly.', 'Restarting it brings the engine data back.', 'demo-engine exited (signal: 6)', 'sd.engine.stopped');

const health = (revision: number, engine: ReturnType<typeof service>) => ({
  revision,
  services: [engine, service('sd.index', 'the search index', 'ok')],
});

const NO_NOTICES = { revision: 1, notices: [], unread: 0 };

/** A `paddock://jobs` snapshot of the demo's import, as `paddock-jobs` serialises it. */
const job = (seq: number, state: Record<string, unknown>) => ({
  id: 7,
  kind: 'demo.import',
  title: 'Importing logs',
  subject: '3 folders',
  seq,
  elapsedMs: 100 * seq,
  etaMs: null,
  cancellable: true,
  notCancellableReason: null,
  ...state,
});

/** The engine at rest: both services serving, no messages, no jobs. */
const desktop = (page: Page): Promise<TauriFake> =>
  installTauriFake(page, {
    otherwise: 'null',
    replies: {
      shell_version: VERSION_REPLY,
      settings_overview: SETTINGS_OVERVIEW_FIXTURE,
      about_whats_new_start: 'nothing',
      dialog_open: { label: 'd-main-1', dialogId: 'sd.inspector', existing: false },
      health_snapshot: health(1, service('sd.engine', 'the demo engine', 'ok')),
      notifications_list: NO_NOTICES,
      jobs_snapshot: [],
      demo_break: null,
      demo_job_start: 7,
      health_repair: health(3, service('sd.engine', 'the demo engine', 'repairing', ENGINE_DOWN, 'restart')),
      job_cancel: true,
    },
  });

test('a service failure comes from the engine, and Repair asks the shell plugin', async ({ app, page, pageErrors }) => {
  const ipc = await desktop(page);
  const { chip } = app.statusBar;
  await page.goto('/');
  await expect(page.getByRole('heading', { level: 1, name: 'Overview' })).toBeVisible();
  await expect.poll(() => ipc.names()).toContain('health_snapshot');
  await page.getByRole('button', { name: 'Break the demo engine' }).click();
  await expect.poll(async () => (await ipc.callsOf('demo_break')).map((c) => c.args)).toEqual([{ service: 'sd.engine' }]);
  // Nothing changes until Rust reports it.
  await expect(chip).toHaveCount(0);
  expect(await ipc.emit('paddock://service-health', health(2, service('sd.engine', 'the demo engine', 'down', ENGINE_DOWN, 'restart')))).toBeGreaterThan(0);
  await expect(chip).toHaveText('Demo engine: stopped');
  // The engine-bound panel stops showing its data and waits with the chip's cause (a designed state, not stale data).
  await expect(page.getByTestId('waiting-for-sd.engine')).toContainText('Waiting for the demo engine');
  await expect(page.getByTestId('engine-data')).toHaveCount(0);

  await chip.click();
  await app.notifications.services.getByRole('button', { name: 'Restart' }).click();
  await expect.poll(async () => (await ipc.callsOf('health_repair')).map((c) => c.args)).toEqual([{ service: 'sd.engine' }]);
  await ipc.emit('paddock://service-health', health(4, service('sd.engine', 'the demo engine', 'ok')));
  await expect(chip).toHaveCount(0);
  await expect(page.getByTestId('engine-data')).toBeVisible();
  // The browser build's stand-ins were never used: the engine's commands went to Rust only.
  expect(await ipc.names()).not.toContain('plugin:paddock-shell|demo_break');
  expect(pageErrors).toEqual([]);
});

test('a sample job runs in the engine: its phases come as events, Cancel asks the plugin', async ({ app, page, pageErrors }) => {
  const ipc = await desktop(page);
  await page.goto('/');
  await expect.poll(() => ipc.names()).toContain('jobs_snapshot');
  await page.getByRole('button', { name: 'Start sample job' }).click();
  await expect.poll(async () => (await ipc.callsOf('demo_job_start')).map((c) => c.args)).toEqual([{ kind: 'import' }]);

  await ipc.emit('paddock://jobs', job(1, { state: 'queued' }));
  await ipc.emit('paddock://jobs', job(2, { state: 'running', phase: { id: 'find', label: 'Finding files', unit: 'items', total: null, done: 70 } }));
  const status = app.statusBar.jobs;
  // Rule 11: the phase and its value beside the name, a growing count while the total is unknown, then a percentage.
  await expect(status).toHaveText('1 running: Importing logs · Finding files 70 so far');
  await expect(status).toHaveAttribute('title', 'Importing logs: Finding files');
  await ipc.emit('paddock://jobs', job(3, { state: 'running', phase: { id: 'read', label: 'Reading', unit: 'bytes', total: 8_388_608, done: 4_194_304 } }));
  await expect(status).toHaveText('1 running: Importing logs · Reading 50%');
  await status.click();
  const item = app.jobs.items;
  await expect(item).toContainText('Reading');
  await item.getByRole('button', { name: 'Cancel' }).click();
  await expect.poll(async () => (await ipc.callsOf('job_cancel')).map((c) => c.args)).toEqual([{ id: 7 }]);
  await ipc.emit('paddock://jobs', job(4, { state: 'cancelled', reason: 'user', cancellable: false }));
  await expect(item).toContainText('Cancelled');
  await expect(status).toHaveText('Jobs');
  expect(pageErrors).toEqual([]);
});

test('dialogs open as windows of the shell plugin, not in the page', async ({ app, page }) => {
  const ipc = await desktop(page);
  await page.goto('/#/dialogs');
  await page.locator('button[data-dialog-open="sd.inspector"]').click();
  await expect.poll(async () => (await ipc.callsOf('dialog_open')).length).toBe(1);
  const [open] = await ipc.callsOf('dialog_open');
  expect((open?.args as { request: { dialogId: string } }).request.dialogId).toBe('sd.inspector');
  await expect(app.dialogs.inPage('Engine inspector')).toHaveCount(0);
});

test('a dialog window takes its input from the plugin and answers through it', async ({ app, page }) => {
  const ipc = await installTauriFake(page, {
    otherwise: 'null',
    replies: {
      dialog_take_input: { dialogId: 'sd.rename', title: 'Rename', kind: 'form', modality: 'modal', requiredChoice: false, input: { current: 'Greeting' } },
      dialog_close: null,
      dialog_fit_height: null,
    },
  });
  await app.dialogs.openWindow('sd.rename', { current: 'Greeting' });
  await app.dialogs.loaded();
  await expect.poll(() => ipc.names()).toContain('dialog_take_input');
  await expect(app.dialogs.shell.getByRole('textbox')).toHaveValue('Greeting');
  // Window events are listened to on this window only (`getCurrentWebviewWindow().listen`).
  const listens = (await ipc.callsOf('plugin:event|listen')).map((c) => c.args as { target: { kind: string } });
  expect(listens.some((l) => l.target.kind === 'WebviewWindow')).toBe(true);
  await app.dialogs.shell.getByRole('textbox').fill('Hello');
  await app.dialogs.shell.getByRole('button', { name: 'Rename' }).click();
  await expect.poll(async () => (await ipc.callsOf('dialog_close')).map((c) => c.args)).toEqual([{ result: 'Hello' }]);
  // Nothing went to the browser build's answer store.
  expect(await app.dialogs.result()).toBeNull();
});

test.describe('tray popover', () => {
  const ENGINE_PROBLEM = service('sd.engine', 'the demo engine', 'down', ENGINE_DOWN, 'restart');
  const RUNNING = job(5, { state: 'running', phase: { id: 'read', label: 'Reading', unit: 'bytes', total: 8_388_608, done: 2_097_152 } });
  const tray = (page: Page, state: unknown): Promise<TauriFake> =>
    installTauriFake(page, {
      replies: {
        demo_tray_state: state,
        tray_fit_height: 180,
        tray_close: null,
        window_focus: null,
        demo_tray_repair: null,
        app_quit: null,
      },
    });

  test('calm: the app icon, name and status, no jobs, and Open window closes it', async ({ page, pageErrors }) => {
    const ipc = await tray(page, { problems: [], job: null });
    await page.goto('/#/tray');
    const popover = page.getByRole('dialog', { name: 'Paddock Shell Demo' });
    await expect(popover).toBeVisible();
    await expect(page.getByTestId('tray-status')).toHaveText('Running');
    await expect(page.getByTestId('tray-idle')).toHaveText('No jobs running');
    await expect(page.getByTestId('tray-attention')).toHaveCount(0);
    await expect.poll(async () => (await ipc.callsOf('tray_fit_height')).length).toBeGreaterThan(0);
    const [fit] = await ipc.callsOf('tray_fit_height');
    expect((fit?.args as { height: number }).height).toBeGreaterThan(0);
    // It follows the engine: it listens to the health and jobs events.
    const listened = (await ipc.callsOf('plugin:event|listen')).map((c) => (c.args as { event: string }).event);
    expect(listened).toEqual(expect.arrayContaining(['paddock://service-health', 'paddock://jobs']));

    await page.getByRole('button', { name: 'Open window' }).click();
    await expect.poll(async () => (await ipc.callsOf('window_focus')).map((c) => c.args)).toEqual([{ label: 'main' }]);
    await expect.poll(() => ipc.names()).toContain('tray_close');
    // Only commands open to the tray class: no start-up contract, no health, no settings.
    for (const n of new Set(await ipc.names())) expect(['demo_tray_state', 'tray_fit_height', 'tray_close', 'window_focus', 'plugin:event|listen']).toContain(n);
    expect(pageErrors).toEqual([]);
    expect(await axeViolations(page)).toEqual([]);
  });

  test('a stopped service: the pill says so, the banner repairs it as the chip does, and the job shows phase and value', async ({ page }) => {
    const ipc = await tray(page, { problems: [ENGINE_PROBLEM], job: RUNNING });
    await page.goto('/#/tray');
    await expect(page.getByTestId('tray-status')).toHaveText('Demo engine stopped');
    const banner = page.getByTestId('tray-attention');
    await expect(banner).toContainText('The demo engine stopped unexpectedly.');
    await expect(page.getByTestId('tray-job')).toContainText('Importing logs');
    await expect(page.getByTestId('tray-job')).toContainText('Reading 25%');
    await banner.getByRole('button', { name: 'Restart demo engine' }).click();
    await expect.poll(async () => (await ipc.callsOf('demo_tray_repair')).map((c) => c.args)).toEqual([{ service: 'sd.engine' }]);
    // A repair keeps the popover open (it shows the outcome when the engine's event comes).
    expect(await ipc.names()).not.toContain('tray_close');
    const before = (await ipc.callsOf('demo_tray_state')).length;
    await ipc.emit('paddock://service-health', health(5, service('sd.engine', 'the demo engine', 'ok')));
    await expect.poll(async () => (await ipc.callsOf('demo_tray_state')).length).toBeGreaterThan(before);
    expect(await axeViolations(page)).toEqual([]);
  });

  test('a refused action stays on screen with its reason', async ({ page }) => {
    const ipc = await installTauriFake(page, { replies: { demo_tray_state: { problems: [], job: null }, tray_fit_height: 180, tray_close: null } });
    await page.goto('/#/tray');
    await page.getByRole('button', { name: 'Start sample job' }).click();
    await expect(page.getByRole('alert')).toContainText('That did not work: demo_job_start is unavailable in this test');
    expect(await ipc.names()).not.toContain('tray_close');
  });
});
