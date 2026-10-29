import { Injector } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { AppearanceStore, BrowserAppearanceBackend, buildManifest, CommandService, ContributionRegistry, manifestDrift, provideAppearance, serializeManifest, SHELL_COMMANDS } from '@decryp7/paddock-ui/shell';
import { provideMenus, provideSubmenus } from '@decryp7/paddock-ui/shell/menu';
import { SETTINGS_CONTRIBUTIONS } from '@decryp7/paddock-ui/shell/settings';
import { assertContributionsValid, dialogTableProblems, provideShellTesting } from '@decryp7/paddock-ui/shell/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { APP_COMMANDS } from '../generated/app-commands';
import { DIALOG_BASE } from '../generated/dialog-contracts';
import { ABOUT_CONTRIBUTIONS, CONTRIBUTIONS, DIALOGS } from './contributions';
import { dynoContext } from './context';
import { DIALOG_BASE_COMMANDS } from './dialog-base';
import { NAV } from './nav';

/** Node's `fs`, loaded at run time (the spec programme has no Node types). */
interface NodeFs {
  readFileSync(path: string, encoding: 'utf8'): string;
  writeFileSync(path: string, data: string): void;
}
const nodeFs = (): Promise<NodeFs> => import(/* @vite-ignore */ ['node', 'fs'].join(':')) as Promise<NodeFs>;
const env = (name: string): string | undefined => (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env[name];

/** Rust reads this file (`paddock_shell::Builder::problems`, `app/src-tauri/tests/shell.rs`). */
const MANIFEST = 'paddock-manifest.json';

function configure(contributions = [CONTRIBUTIONS]): MockBackend {
  const backend = new MockBackend({ latencyMs: 0, now: () => new Date(2026, 9, 22, 12).getTime() });
  TestBed.configureTestingModule({
    providers: [
      { provide: BACKEND, useValue: backend },
      provideRouter([]),
      ...provideShellTesting({
        contributions,
        context: () => Promise.resolve(dynoContext),
        providers: [provideMenus({ appName: 'Dyno' }), provideSubmenus(), provideAppearance({ accent: 'dyno', backend: () => new BrowserAppearanceBackend({ storage: null, channel: null }) })],
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

describe('Dyno manifest', () => {
  it('keeps paddock-manifest.json equal to the main window’s contributions', async () => {
    configure([CONTRIBUTIONS, ABOUT_CONTRIBUTIONS, SETTINGS_CONTRIBUTIONS] as never);
    const manifest = buildManifest(TestBed.inject(ContributionRegistry));
    expect([...manifest.healthServices].sort()).toEqual(['dy.agent']);
    const fs = await nodeFs();
    if (env('PADDOCK_WRITE_MANIFEST') === '1') fs.writeFileSync(MANIFEST, serializeManifest(manifest));
    expect(manifestDrift(manifest, fs.readFileSync(MANIFEST, 'utf8'))).toEqual([]);
  });
});
