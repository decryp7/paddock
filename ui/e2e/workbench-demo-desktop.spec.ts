import { expect, installTauriFake, readVersion, test, WorkbenchApp, type IpcCall, type Page, type TauriFake } from '@decryp7/paddock-e2e';

// The workbench demo as the desktop app (plan X50 W8, apps/workbench-demo): every part on its Tauri backend, played by the
// kit's fake `__TAURI_INTERNALS__` (@decryp7/paddock-e2e). The shell's commands go to `paddock-shell` (the kit records them
// by their plain names), the workbench's to `paddock-workbench` (recorded as `plugin:paddock-workbench|<command>`), the
// app's own (`pitwall_state`) as they are; the app's Rust tests check the other side (apps/workbench-demo/src-tauri/tests/app.rs).

const WORKBENCH = 'plugin:paddock-workbench|';
const wb = (command: string): string => `${WORKBENCH}${command}`;

/** The shell's version (the plugin answers the UI package's) and the app's own: each from its single source (rule 6). */
const SHELL_VERSION = readVersion('projects/paddock-ui/package.json');
const APP_VERSION = readVersion('../apps/workbench-demo/src-tauri/Cargo.toml');
const VERSION_REPLY = { shell: SHELL_VERSION, app: { id: 'workbench-demo', name: 'Paddock Workbench Demo', version: APP_VERSION } };

/** What `pitwall_state` answers while Pitwall is not running (the calm state, with its fix). */
const PITWALL_NOT_RUNNING = {
  state: { kind: 'notRunning' },
  app: 'Workbench demo',
  message: 'Pitwall is not running.',
  actions: ['startPitwall'],
  pairing: null,
  builtInAvailable: false,
  builtInForSession: false,
};

const ASSISTANT_INFO = { provider: 'Pitwall', tools: [{ name: 'list_files', description: 'Lists the sample repository', write: false, destructive: false }] };

/** The engine at rest: nothing saved in the layout store, no jobs, no messages, Pitwall not running. */
const desktop = (page: Page, extra: Record<string, unknown> = {}): Promise<TauriFake> =>
  installTauriFake(page, {
    otherwise: 'null',
    replies: {
      shell_version: VERSION_REPLY,
      [wb('layout_get')]: null,
      [wb('layout_set')]: null,
      [wb('assistant_info')]: ASSISTANT_INFO,
      [wb('assistant_conversations')]: [],
      [wb('panels_list')]: [],
      pitwall_state: PITWALL_NOT_RUNNING,
      health_snapshot: { revision: 1, services: [] },
      notifications_list: { revision: 1, notices: [], unread: 0 },
      jobs_snapshot: [],
      ...extra,
    },
  });

/** The commands called so far, as a set. */
const called = async (ipc: TauriFake): Promise<Set<string>> => new Set(await ipc.names());


const argsOf = (calls: IpcCall[]): unknown[] => calls.map((c) => c.args);

test('the main window boots on the Tauri backends: the plugin layout store and the shell parts', async ({ page, pageErrors }) => {
  const ipc = await desktop(page);
  await page.goto('/');
  await expect(page.locator('pd-workbench')).toBeVisible();
  // The dock reads its saved layout from the engine, by the plugin's command and the app's key (never the browser's storage).
  const keys = async (): Promise<string[]> => argsOf(await ipc.callsOf(wb('layout_get'))).map((a) => (a as { key: string }).key);
  await expect.poll(keys).toEqual(expect.arrayContaining(['wd.layouts', 'wd.perspectives', 'wd.layout']));
  // The shell's parts are on their backends: start-up, health, notifications, jobs.
  for (const n of ['window_ready', 'health_snapshot', 'notifications_list', 'jobs_snapshot']) await expect.poll(() => called(ipc), n).toContain(n);
  // A change of the arrangement is saved to the engine (debounced), by `layout_set` under the window's key.
  const app = new WorkbenchApp(page);
  await app.explorer.node('wd.files', 'README.md').dblclick();
  await expect.poll(async () => argsOf(await ipc.callsOf(wb('layout_set'))).map((a) => (a as { key: string }).key)).toContain('wd.layout');
  // The browser's storage was never the layout store.
  expect(await page.evaluate(() => Object.keys(localStorage).filter((k) => k.startsWith('wd')))).toEqual([]);
  expect(pageErrors).toEqual([]);
});

test('the assistant pane asks the workbench plugin and Pitwall asks the app, not the browser stand-ins', async ({ page, pageErrors }) => {
  const ipc = await desktop(page);
  await page.goto('/');
  await expect(page.locator('pd-workbench')).toBeVisible();
  await expect.poll(async () => (await ipc.callsOf(wb('assistant_info'))).length).toBeGreaterThan(0);
  await expect.poll(async () => (await ipc.callsOf('pitwall_state')).length).toBeGreaterThan(0);
  // The workbench's commands carry the plugin's prefix; none went out plain.
  expect(await ipc.names()).not.toContain('assistant_info');
  expect(await ipc.names()).not.toContain('layout_get');
  expect(pageErrors).toEqual([]);
});

test('a pop-out window boots the pop-out shell through the panel layer, with its own small provider set', async ({ page, pageErrors }) => {
  const ipc = await desktop(page);
  // What Rust loads for a pop-out (`paddock_panels::popout_url`): the panel, its host window and its own label.
  await page.goto('/#/popout?panel=wd.orientation&host=main&label=p-main-1');
  await expect(page.getByRole('button', { name: 'Dock back' })).toBeVisible();
  await expect(page.locator('pd-workbench')).toHaveCount(0);
  // It tells the host window it is there (`ui_sync_publish`, the whole shared state), listening to this window's events only.
  await expect.poll(async () => (await ipc.callsOf(wb('ui_sync_publish'))).length).toBeGreaterThan(0);
  const [hello] = argsOf(await ipc.callsOf(wb('ui_sync_publish')));
  expect(hello).toMatchObject({ sync: { scope: 'full', message: { t: 'hello', label: 'p-main-1', host: false } } });
  const listens = argsOf(await ipc.callsOf('plugin:event|listen')) as { event: string; target: { kind: string } }[];
  expect(listens.find((l) => l.event === 'paddock://ui-sync')?.target.kind).toBe('WebviewWindow');
  // The frameless title bar's window controls go to the window plugin, and the window reports ready (ADR 0058).
  for (const n of ['window_ready', 'plugin:window|is_maximized']) await expect.poll(() => called(ipc), n).toContain(n);
  const names = await called(ipc);
  // None of the host-only parts started: no dock layouts, no assistant, no Pitwall, no jobs.
  for (const n of [wb('layout_get'), wb('assistant_info'), 'pitwall_state', 'jobs_snapshot']) expect(names, n).not.toContain(n);
  expect(pageErrors).toEqual([]);
});

test('a dialog window boots the dialog shell and shares the display state; one the app does not know ends in a designed error', async ({ app, page, pageErrors }) => {
  const ipc = await desktop(page, { dialog_close: null, dialog_fit_height: null });
  // The demo has no dialogs of its own yet (`bootDialogWindow`'s `dialogs` is empty): any id is unknown to it.
  await app.dialogs.openWindow('wd.nothing', {});
  await expect(app.dialogs.shell).toContainText("This window couldn't open");
  await expect(app.dialogs.shell).toContainText('This dialog is not known to this version of the app: wd.nothing');
  await expect(page.locator('pd-workbench')).toHaveCount(0);
  // Display state (theme, formats) is shared with the host by the panel layer, in the `display` scope.
  await expect.poll(async () => (await ipc.callsOf(wb('ui_sync_publish'))).length).toBeGreaterThan(0);
  const [hello] = argsOf(await ipc.callsOf(wb('ui_sync_publish')));
  expect(hello).toMatchObject({ sync: { scope: 'display', message: { t: 'hello', label: 'd-main-1', host: false } } });
  for (const n of ['window_ready', 'plugin:event|listen']) await expect.poll(() => called(ipc), n).toContain(n);
  // Its way out asks the shell plugin to close this window.
  await app.dialogs.wayOut.click();
  await expect.poll(async () => (await ipc.callsOf('dialog_close')).length).toBe(1);
  expect(pageErrors).toEqual([]);
});
