import { Injector } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { BrowserAppearanceBackend, buildManifest, CommandService, ContributionRegistry, manifestDrift, provideAppearance, serializeManifest, SHELL_COMMANDS, ShellContextHost } from '@decryp7/paddock-ui/shell';
import { provideMenus, provideSubmenus, QUIT_CONTRIBUTIONS } from '@decryp7/paddock-ui/shell/menu';
import { SETTINGS_CONTRIBUTIONS } from '@decryp7/paddock-ui/shell/settings';
import { assertContributionsValid, dialogTableProblems, provideShellTesting } from '@decryp7/paddock-ui/shell/testing';
import { BACKEND } from '../core/backend';
import { LiveStore } from '../core/live.store';
import { MockBackend } from '../core/mock-backend';
import { PITWALL_APP_QUIT, QuitFlow } from '../features/quit/quit-flow';
import { APP_COMMANDS } from '../generated/app-commands';
import { DIALOG_BASE } from '../generated/dialog-contracts';
import { ABOUT_CONTRIBUTIONS, CONTRIBUTIONS, DIALOGS } from './contributions';
import { pitwallContext } from './context';
import { DIALOG_BASE_COMMANDS } from './dialog-base';
import { NAV } from './nav';

/** Node's `fs`, loaded at run time (the spec programme has no Node types). */
interface NodeFs {
  readFileSync(path: string, encoding: 'utf8'): string;
  writeFileSync(path: string, data: string): void;
}
const nodeFs = (): Promise<NodeFs> => import(/* @vite-ignore */ ['node', 'fs'].join(':')) as Promise<NodeFs>;
const env = (name: string): string | undefined => (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env[name];

/** The shell's Quit command, which the frames contribute; Pitwall only provides what it does (`PITWALL_APP_QUIT`). */
const QUIT_COMMAND = 'pd.app.quit';

/** Rust reads this file (`paddock_shell::Builder::problems`, `app/src-tauri/tests/shell.rs`). */
const MANIFEST = 'paddock-manifest.json';

function configure(contributions = [CONTRIBUTIONS, QUIT_CONTRIBUTIONS], backend = new MockBackend()): MockBackend {
  TestBed.configureTestingModule({
    providers: [
      { provide: BACKEND, useValue: backend },
      provideRouter([]),
      ...provideShellTesting({
        contributions,
        context: () => Promise.resolve(pitwallContext),
        providers: [PITWALL_APP_QUIT, provideMenus({ appName: 'Pitwall' }), provideSubmenus(), provideAppearance({ accent: 'pitwall', backend: () => new BrowserAppearanceBackend({ storage: null, channel: null }) })],
      }),
    ],
  });
  return backend;
}

describe('Pitwall contributions', () => {
  it('pass assertContributionsValid', () => {
    configure();
    expect(() => assertContributionsValid(TestBed.inject(Injector))).not.toThrow();
  });

  it('list every screen of the old sidebar as a page, in order, and Settings is not one of them', () => {
    configure();
    const pages = TestBed.inject(ContributionRegistry).pages();
    expect(pages.map((p) => p.title.text)).toEqual(NAV.map((n) => n.label));
    expect(pages.map((p) => p.id)).toEqual(['pw.dashboard', 'pw.models', 'pw.engines', 'pw.apps', 'pw.requests', 'pw.playground', 'pw.benchmarks']);
    expect(pages.some((p) => p.id.includes('settings'))).toBe(false);
  });

  it('badge Apps & keys with the waiting pairings and Models with the running downloads', async () => {
    configure();
    const live = TestBed.inject(LiveStore);
    await live.load();
    const ctx = await TestBed.inject(ShellContextHost).load();
    const pages = TestBed.inject(ContributionRegistry).pages();
    const apps = pages.find((p) => p.id === 'pw.apps')!;
    const models = pages.find((p) => p.id === 'pw.models')!;
    expect(apps.badge!(ctx)).toBe(live.pairings().length || null);
    expect(apps.badgeLabel!(ctx)).toMatch(/pairing request/);
    expect(models.badgeLabel!(ctx)).toMatch(/download/);
    expect(pages.find((p) => p.id === 'pw.dashboard')!.badge).toBeUndefined();
  });

  it('pause, resume and release memory through the live store, offered only when they apply', async () => {
    const backend = configure();
    const live = TestBed.inject(LiveStore);
    await live.load();
    live.connect();
    await TestBed.inject(ShellContextHost).load();
    const commands = TestBed.inject(CommandService);
    const offered = (id: string) => commands.available().some((c) => c.id === id);
    expect(offered('pw.pause-serving')).toBe(true);
    expect(offered('pw.resume-serving')).toBe(false);
    const pause = vi.spyOn(backend, 'setServing');
    expect(await commands.run('pw.pause-serving')).toBe(true);
    expect(pause).toHaveBeenCalledWith(true);
    expect(live.paused()).toBe(true);
    expect(offered('pw.resume-serving')).toBe(true);
    expect(await commands.run('pw.resume-serving')).toBe(true);
    expect(pause).toHaveBeenLastCalledWith(false);
    const release = vi.spyOn(backend, 'releaseMemory');
    expect(await commands.run('pw.release-memory')).toBe(true);
    expect(release).toHaveBeenCalled();
    live.disconnect();
  });

  it('quit through Pitwall’s quit flow: the shell’s Quit asks the setting, "and stop serving" is forced', async () => {
    configure();
    await TestBed.inject(ShellContextHost).load();
    const quit = vi.spyOn(TestBed.inject(QuitFlow), 'quit').mockResolvedValue();
    const commands = TestBed.inject(CommandService);
    expect(await commands.run(QUIT_COMMAND)).toBe(true);
    expect(quit).toHaveBeenLastCalledWith(null);
    expect(await commands.run('pw.quit-stop')).toBe(true);
    expect(quit).toHaveBeenLastCalledWith('stopServing');
  });

  it('has the main window provide PITWALL_APP_QUIT (without it the shell’s Quit would skip the quit question)', async () => {
    // The window's providers are built in `bootMainWindow`, which cannot run here: check the registration in its source.
    const fs = await nodeFs();
    const source = fs.readFileSync('src/app/shell/main-window.ts', 'utf8');
    expect(source).toMatch(/^\s+PITWALL_APP_QUIT,$/m);
  });

  it('title every own command in sentence case, under a category', () => {
    configure();
    const own = TestBed.inject(ContributionRegistry)
      .commands()
      .filter((c) => c.id.startsWith('pw.'));
    expect(own.map((c) => c.id)).toEqual(['pw.pause-serving', 'pw.resume-serving', 'pw.release-memory', 'pw.quit-stop']);
    for (const c of own) {
      expect(c.category).toBeDefined();
      expect(c.title.text).toMatch(/^[A-Z][a-z]/);
    }
  });
});

describe('Pitwall dialogs', () => {
  it('only name commands that exist and are open to dialog windows', () => {
    expect(dialogTableProblems(DIALOGS, { ...APP_COMMANDS, ...SHELL_COMMANDS }, DIALOG_BASE_COMMANDS)).toEqual([]);
  });

  it('take the base commands from the Rust table', () => {
    expect([...DIALOG_BASE]).toEqual([...DIALOG_BASE_COMMANDS].sort());
  });

  it('let Pitwall’s own dialogs call nothing: the window that opened them acts on the answer', () => {
    for (const d of DIALOGS.filter((x) => x.id.startsWith('pw.'))) expect(d.commands, d.id).toEqual([]);
  });
});

describe('Pitwall manifest', () => {
  it('keeps paddock-manifest.json equal to the main window’s contributions', async () => {
    configure([CONTRIBUTIONS, QUIT_CONTRIBUTIONS, ABOUT_CONTRIBUTIONS, SETTINGS_CONTRIBUTIONS] as never);
    const manifest = buildManifest(TestBed.inject(ContributionRegistry));
    expect([...manifest.healthServices]).toEqual([]);
    const fs = await nodeFs();
    if (env('PADDOCK_WRITE_MANIFEST') === '1') fs.writeFileSync(MANIFEST, serializeManifest(manifest));
    expect(manifestDrift(manifest, fs.readFileSync(MANIFEST, 'utf8'))).toEqual([]);
  });
});
