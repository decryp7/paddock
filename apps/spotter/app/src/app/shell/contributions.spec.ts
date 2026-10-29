import { Injector } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { AppearanceStore, BrowserAppearanceBackend, buildManifest, CommandService, ContributionRegistry, manifestDrift, provideAppearance, serializeManifest } from '@decryp7/paddock-ui/shell';
import { provideMenus, provideSubmenus } from '@decryp7/paddock-ui/shell/menu';
import { SETTINGS_CONTRIBUTIONS } from '@decryp7/paddock-ui/shell/settings';
import { assertContributionsValid, dialogTableProblems, provideShellTesting, shellPorts } from '@decryp7/paddock-ui/shell/testing';
import { SHELL_COMMANDS } from '@decryp7/paddock-ui/shell';
import { ABOUT_CONTRIBUTIONS, CONTRIBUTIONS, DIALOGS } from './contributions';
import { spotterContext } from './context';
import { DIALOG_BASE_COMMANDS } from './dialog-base';
import { APP_COMMANDS } from '../generated/app-commands';
import { DIALOG_BASE } from '../generated/dialog-contracts';
import { BACKEND } from '../core/backend';
import { ClockService } from '../core/clock.service';
import { MockBackend } from '../core/mock-backend';
import { SettingsStore } from '../state/settings.store';
import { WatchesStore } from '../state/watches.store';
import { provideRouter } from '@angular/router';

/** Node's `fs`, loaded at run time (the spec programme has no Node types). */
interface NodeFs {
  readFileSync(path: string, encoding: 'utf8'): string;
  writeFileSync(path: string, data: string): void;
}
const nodeFs = (): Promise<NodeFs> => import(/* @vite-ignore */ ['node', 'fs'].join(':')) as Promise<NodeFs>;
const env = (name: string): string | undefined => (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env[name];

/** Rust reads this file (`paddock_shell::Builder::problems`, `app/src-tauri/tests/shell.rs`). */
const MANIFEST = 'paddock-manifest.json';

function configure(contributions = [CONTRIBUTIONS]): void {
  const backend = new MockBackend({ now: () => new Date(2026, 9, 22, 12).getTime() });
  TestBed.configureTestingModule({
    providers: [
      { provide: BACKEND, useValue: backend },
      provideRouter([]),
      ...provideShellTesting({
        contributions,
        context: () => Promise.resolve(spotterContext),
        providers: [provideMenus({ appName: 'Spotter' }), provideSubmenus(), provideAppearance({ accent: 'spotter', backend: () => new BrowserAppearanceBackend({ storage: null, channel: null }) })],
      }),
    ],
  });
}

describe('Spotter contributions', () => {
  beforeEach(() => {
    configure();
    // The mock backend's clock and the app's clock agree.
    TestBed.inject(ClockService).set(new Date(2026, 9, 22, 12).getTime());
  });

  it('pass assertContributionsValid', () => {
    expect(() => assertContributionsValid(TestBed.inject(Injector))).not.toThrow();
  });

  it('give every command a working handler against the context', async () => {
    const commands = TestBed.inject(CommandService);
    const watches = TestBed.inject(WatchesStore);
    await watches.load();
    await expect(commands.run('sp.check-all')).resolves.toBe(true);
    await TestBed.inject(SettingsStore).loadSettings();
    await expect(commands.run('sp.pause-notifications')).resolves.toBe(true);
    await expect(commands.run('sp.resume-notifications')).resolves.toBe(true);
    expect(shellPorts().messages().length).toBeGreaterThan(0);
  });

  it('offer Resume notifications only while they are paused', async () => {
    const commands = TestBed.inject(CommandService);
    await TestBed.inject(SettingsStore).loadSettings();
    await expect(commands.run('sp.resume-notifications')).resolves.toBe(false);
    await commands.run('sp.pause-notifications');
    await expect(commands.run('sp.resume-notifications')).resolves.toBe(true);
    await expect(commands.run('sp.resume-notifications')).resolves.toBe(false);
  });

  it('do not offer Mark all as seen when nothing is unseen', async () => {
    const commands = TestBed.inject(CommandService);
    await TestBed.inject(WatchesStore).load();
    const unseen = TestBed.inject(WatchesStore).unseen().length;
    await expect(commands.run('sp.mark-all-seen')).resolves.toBe(unseen > 0);
  });

  it('keep the appearance the shell’s', () => {
    expect(TestBed.inject(AppearanceStore).theme()).toBeDefined();
  });
});

describe('Spotter dialogs', () => {
  it('only name commands that exist and are open to dialog windows', () => {
    const problems = dialogTableProblems(DIALOGS, { ...APP_COMMANDS, ...SHELL_COMMANDS }, DIALOG_BASE_COMMANDS);
    expect(problems).toEqual([]);
  });

  it('take the base commands from the Rust table', () => {
    expect([...DIALOG_BASE]).toEqual([...DIALOG_BASE_COMMANDS].sort());
  });
});

describe('Spotter manifest', () => {
  it('keeps paddock-manifest.json equal to the main window’s contributions', async () => {
    configure([CONTRIBUTIONS, ABOUT_CONTRIBUTIONS, SETTINGS_CONTRIBUTIONS] as never);
    const manifest = buildManifest(TestBed.inject(ContributionRegistry));
    expect([...manifest.healthServices].sort()).toEqual(['sp.renderer', 'sp.scheduler', 'sp.sources']);
    const fs = await nodeFs();
    if (env('PADDOCK_WRITE_MANIFEST') === '1') fs.writeFileSync(MANIFEST, serializeManifest(manifest));
    expect(manifestDrift(manifest, fs.readFileSync(MANIFEST, 'utf8'))).toEqual([]);
  });
});

