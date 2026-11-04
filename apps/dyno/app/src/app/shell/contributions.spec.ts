import { Injector } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { AppearanceStore, BrowserAppearanceBackend, buildManifest, CommandService, ContributionRegistry, manifestDrift, ShellContextHost, provideAppearance, serializeManifest, SHELL_COMMANDS } from '@decryp7/paddock-ui/shell';
import { AppMenu, MENU_MODE, type MenuMode, provideMenus, provideNativeMenu, provideSubmenus, toNativeModel } from '@decryp7/paddock-ui/shell/menu';
import { SETTINGS_CONTRIBUTIONS } from '@decryp7/paddock-ui/shell/settings';
import { assertContributionsValid, dialogTableProblems, provideShellTesting } from '@decryp7/paddock-ui/shell/testing';
import { BACKEND } from '../core/backend';
import type { MockBackend } from '../core/mock-backend';
import { jobsProviders, mockWithJobs } from '../../testing/job-test-support';
import { APP_COMMANDS } from '../generated/app-commands';
import { DIALOG_BASE } from '../generated/dialog-contracts';
import { ABOUT_CONTRIBUTIONS, CONTRIBUTIONS, DIALOGS } from './contributions';
import { dynoContext } from './context';
import { DIALOG_BASE_COMMANDS } from './dialog-base';
import { NAV } from './nav';
import { ReportsStore } from '../state/reports.store';
import { DiagnosticsStore } from '../state/diagnostics.store';
import { SettingsStore } from '../state/settings.store';
import { ShellPages } from '@decryp7/paddock-ui/shell/pages';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { BackupStore } from '../state/backup.store';
import { DiagnosisStore } from '../state/diagnosis.store';
import { HistoryStore } from '../state/history.store';
import { IssuesStore } from '../state/issues.store';

/** Node's `fs`, loaded at run time (the spec programme has no Node types). */
interface NodeFs {
  readFileSync(path: string, encoding: 'utf8'): string;
  writeFileSync(path: string, data: string): void;
}
const nodeFs = (): Promise<NodeFs> => import(/* @vite-ignore */ ['node', 'fs'].join(':')) as Promise<NodeFs>;
const env = (name: string): string | undefined => (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env[name];

/** Rust reads this file (`paddock_shell::Builder::problems`, `app/src-tauri/tests/shell.rs`). */
const MANIFEST = 'paddock-manifest.json';

/** The desktop build's native menu port (what the frames get from `tauriNativeMenuPort(ipc)`): it can quit, so the frame adds Quit. */
const QUITTING_PORT = { setAppMenu: () => Promise.resolve(), onMenu: () => Promise.resolve(() => undefined), quitApp: () => Promise.resolve() };

function configure(contributions = [CONTRIBUTIONS], menuMode: MenuMode = 'in-app'): MockBackend {
  const { mock: backend, jobs } = mockWithJobs({ now: () => new Date(2026, 9, 22, 12).getTime() });
  TestBed.configureTestingModule({
    providers: [
      { provide: BACKEND, useValue: backend },
      provideRouter([]),
      jobsProviders(jobs),
      { provide: MENU_MODE, useValue: menuMode },
      ...provideShellTesting({
        contributions,
        context: () => Promise.resolve(dynoContext),
        providers: [provideMenus({ appName: 'Dyno' }), provideSubmenus(), provideNativeMenu({ port: QUITTING_PORT }), provideAppearance({ accent: 'dyno', backend: () => new BrowserAppearanceBackend({ storage: null, channel: null }) })],
      }),
    ],
  });
  return backend;
}

describe('Dyno contributions', () => {
  beforeEach(() => configure());

  it('pass assertContributionsValid', () => {
    expect(() => assertContributionsValid(TestBed.inject(Injector))).not.toThrow();
  });

  it('list every screen of the sidebar as a page, in order, and Settings is not one of them', () => {
    const pages = TestBed.inject(ContributionRegistry).pages();
    expect(pages.map((p) => p.title.text)).toEqual(NAV.map((n) => n.label));
    expect(pages.some((p) => p.id.includes('settings'))).toBe(false);
  });

  it('give the capture command a working handler against the context', async () => {
    await expect(TestBed.inject(CommandService).run('dy.capture')).resolves.toBe(true);
  });

  it('run every palette command on its page: show the page, then do what the page’s button does', async () => {
    const commands = TestBed.inject(CommandService);
    const go = vi.spyOn(TestBed.inject(ShellPages), 'go').mockResolvedValue(true);
    const history = vi.spyOn(TestBed.inject(HistoryStore), 'load');
    const rescan = vi.spyOn(TestBed.inject(IssuesStore), 'rescan');
    const exportIssues = vi.spyOn(TestBed.inject(IssuesStore), 'exportForDebrief');
    const check = vi.spyOn(TestBed.inject(BackupStore), 'check');
    const run = vi.spyOn(TestBed.inject(DiagnosisStore), 'run');
    const exportReport = vi.spyOn(TestBed.inject(ReportsStore), 'export');
    const save = vi.spyOn(TestBed.inject(DiagnosticsStore), 'save');
    await TestBed.inject(ShellContextHost).load();
    const cases: [string, string, { mock: { calls: unknown[][] } }][] = [
      ['dy.refresh-history', 'dy.dashboard', history],
      ['dy.rescan-issues', 'dy.issues', rescan],
      ['dy.export-issues', 'dy.issues', exportIssues],
      ['dy.check-backup', 'dy.backup', check],
      ['dy.run-why-slow', 'dy.why-slow', run],
      ['dy.export-report', 'dy.reports', exportReport],
      ['dy.save-diagnostics', 'dy.diagnostics', save],
    ];
    for (const [command, page, action] of cases) {
      go.mockClear();
      const before = action.mock.calls.length;
      expect(await commands.run(command), command).toBe(true);
      expect(go, command).toHaveBeenCalledWith(page);
      expect(action.mock.calls.length, command).toBeGreaterThan(before);
    }
    expect(exportReport).toHaveBeenCalledWith('markdown');
  });

  it('open the new-rule window on the alarms page, reading the settings first when they are missing', async () => {
    const commands = TestBed.inject(CommandService);
    const go = vi.spyOn(TestBed.inject(ShellPages), 'go').mockResolvedValue(true);
    const open = vi.spyOn(TestBed.inject(DialogService), 'open').mockReturnValue({ closed: Promise.resolve({ kind: 'dismissed' }) } as never);
    const settings = TestBed.inject(SettingsStore);
    const load = vi.spyOn(settings, 'load');
    await TestBed.inject(ShellContextHost).load();
    expect(settings.settings()).toBeNull();
    expect(await commands.run('dy.add-alarm-rule')).toBe(true);
    expect(load).toHaveBeenCalledTimes(1);
    expect(go).toHaveBeenCalledWith('dy.alarms');
    expect(open).toHaveBeenCalledWith('dy.alert-rule', expect.objectContaining({ inputs: expect.objectContaining({ rule: null }) }));
    // Loaded already: it is not read again.
    expect(await commands.run('dy.add-alarm-rule')).toBe(true);
    expect(load).toHaveBeenCalledTimes(1);
  });

  it('leave the focus on the page when the page was already shown (no navigation moves it)', async () => {
    const main = document.createElement('main');
    main.id = 'pd-main';
    main.tabIndex = -1;
    document.body.append(main);
    vi.spyOn(TestBed.inject(ShellPages), 'go').mockResolvedValue(true);
    await TestBed.inject(ShellContextHost).load();
    expect(await TestBed.inject(CommandService).run('dy.run-why-slow')).toBe(true);
    expect(document.activeElement).toBe(main);
    main.remove();
  });

  it('turn a command off while its page’s button would be off (no second run of a running check)', async () => {
    const commands = TestBed.inject(CommandService);
    await TestBed.inject(ShellContextHost).load();
    const running = TestBed.inject(DiagnosisStore).run();
    expect(commands.isEnabled(commands.find('dy.run-why-slow')!)).toBe(false);
    await running;
    expect(commands.isEnabled(commands.find('dy.run-why-slow')!)).toBe(true);
    const rescan = TestBed.inject(IssuesStore).rescan();
    expect(commands.isEnabled(commands.find('dy.rescan-issues')!)).toBe(false);
    await rescan;
  });

  it('title every palette command in sentence case, under a category', () => {
    const own = TestBed.inject(ContributionRegistry).commands().filter((c) => c.id.startsWith('dy.'));
    expect(own.map((c) => c.id)).toEqual([
      'dy.capture', 'dy.refresh-history', 'dy.rescan-issues', 'dy.export-issues', 'dy.check-backup', 'dy.run-why-slow', 'dy.export-report', 'dy.save-diagnostics', 'dy.add-alarm-rule',
    ]);
    for (const c of own) {
      expect(c.category).toBeDefined();
      expect(c.title.text).toMatch(/^[A-Z][a-z]/);
    }
  });

  it('keep the appearance the shell’s', () => {
    expect(TestBed.inject(AppearanceStore).theme()).toBeDefined();
  });
});

describe('Dyno dialogs', () => {
  it('only name commands that exist and are open to dialog windows', () => {
    expect(dialogTableProblems(DIALOGS, { ...APP_COMMANDS, ...SHELL_COMMANDS }, DIALOG_BASE_COMMANDS)).toEqual([]);
  });

  it('take the base commands from the Rust table', () => {
    expect([...DIALOG_BASE]).toEqual([...DIALOG_BASE_COMMANDS].sort());
  });
});

describe('Dyno menus', () => {
  it('send the native menu bar Quit with Mod+Q, last in the app menu (Cmd+Q works with no window: paddock-menu)', async () => {
    configure([CONTRIBUTIONS, ABOUT_CONTRIBUTIONS, SETTINGS_CONTRIBUTIONS] as never, 'native');
    await TestBed.inject(ShellContextHost).load();
    const app = toNativeModel(TestBed.inject(AppMenu).menus()).menus.find((m) => m.id === 'app');
    const items = (app?.items ?? []).filter((e) => e.kind === 'item');
    expect(items.at(-1)).toMatchObject({ id: 'pd.app.quit', keybinding: 'Mod+Q' });
  });
});

describe('Dyno manifest', () => {
  it('keeps paddock-manifest.json equal to the main window’s contributions', async () => {
    configure([CONTRIBUTIONS, ABOUT_CONTRIBUTIONS, SETTINGS_CONTRIBUTIONS] as never);
    const manifest = buildManifest(TestBed.inject(ContributionRegistry));
    expect([...manifest.healthServices].sort()).toEqual(['dy.agent', 'dy.tray']);
    const fs = await nodeFs();
    if (env('PADDOCK_WRITE_MANIFEST') === '1') fs.writeFileSync(MANIFEST, serializeManifest(manifest));
    expect(manifestDrift(manifest, fs.readFileSync(MANIFEST, 'utf8'))).toEqual([]);
  });
});
