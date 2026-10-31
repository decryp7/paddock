import { Injector } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import {
  AppearanceStore,
  BrowserAppearanceBackend,
  buildManifest,
  ContributionRegistry,
  manifestDrift,
  parseManifest,
  provideAppearance,
  serializeManifest,
  SHELL_COMMANDS,
  type DialogDef,
} from '@decryp7/paddock-ui/shell';
import { provideMenus } from '@decryp7/paddock-ui/shell/menu';
import { SETTINGS_CONTRIBUTIONS } from '@decryp7/paddock-ui/shell/settings';
import { assertContributionsValid, assertDialogBoots, dialogTableProblems, provideShellTesting } from '@decryp7/paddock-ui/shell/testing';
import { APP_COMMANDS } from '../generated/app-commands';
import { DIALOG_BASE } from '../generated/dialog-contracts';
import { DIALOG_BASE_COMMANDS, dialogBaseProviders } from './dialog-base';
import { pitcrewContext } from './pitcrew-context';
import { ABOUT_CONTRIBUTIONS, PITCREW_CONTRIBUTIONS, PITCREW_DIALOGS, PITCREW_SETTINGS_PAGES, SERVICE_ID } from './pitcrew.contributions';
import { backendProviders } from './dialog-providers';

// What every app on the shell runs over its own registry (ADR 0012 §7): the contribution lint, the manifest Rust checks, the
// boot-every-dialog harness and the dialog contract file.
const DIALOGS = PITCREW_DIALOGS as readonly DialogDef[];

/** Node's `fs`, loaded at run time (the spec programme has no Node types). */
interface NodeFs {
  readFileSync(path: string, encoding: 'utf8'): string;
  writeFileSync(path: string, data: string): void;
}
const nodeFs = (): Promise<NodeFs> => import(/* @vite-ignore */ ['node', 'fs'].join(':')) as Promise<NodeFs>;
const env = (name: string): string | undefined => (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env[name];
const MANIFEST = 'src-tauri/paddock-manifest.json';

function configure(): void {
  TestBed.configureTestingModule({
    providers: provideShellTesting({
      contributions: [PITCREW_CONTRIBUTIONS, ABOUT_CONTRIBUTIONS, SETTINGS_CONTRIBUTIONS],
      context: () => Promise.resolve(pitcrewContext),
      providers: [
        provideMenus({ appName: 'Pitcrew' }),
        provideAppearance({ accent: 'pitcrew', backend: () => new BrowserAppearanceBackend({ storage: null, channel: null }) }),
        ...backendProviders(),
      ],
    }),
  });
}

describe('Pitcrew contributions', () => {
  beforeEach(configure);

  it('pass assertContributionsValid', () => {
    expect(() => assertContributionsValid(TestBed.inject(Injector))).not.toThrow();
    expect(TestBed.inject(AppearanceStore)).toBeTruthy();
  });

  it('give every page, the service and its dialogs', () => {
    const registry = TestBed.inject(ContributionRegistry);
    const ids = (kind: string) => registry.items(kind).map((i) => (i as { id: string }).id);
    expect(ids('pages')).toEqual(['pc.dashboard', 'pc.runners', 'pc.activity', 'pc.insights', 'pc.storage', 'pc.logs', 'pc.updates']);
    expect(ids('healthServices')).toEqual([SERVICE_ID]);
    expect(PITCREW_SETTINGS_PAGES.map((p) => p.id)).toEqual(['pc.general', 'pc.service', 'pc.defaults', 'pc.channels', 'pc.tokens', 'pc.diagnostics']);
  });

  it('give a manifest that round-trips', () => {
    const manifest = buildManifest(TestBed.inject(ContributionRegistry));
    expect(manifest.menus.find((m) => m.command === 'pc.register')?.accelerator).toBe('CmdOrCtrl+Shift+N');
    expect(parseManifest(serializeManifest(manifest))).toEqual({ ok: true, manifest });
  });

  // Regenerate with PADDOCK_WRITE_MANIFEST=1 npx ng test --watch=false --include=**/pitcrew.contributions.spec.ts
  it('keep paddock-manifest.json equal to the main window’s contributions (Rust checks it)', async () => {
    const manifest = buildManifest(TestBed.inject(ContributionRegistry));
    const fs = await nodeFs();
    if (env('PADDOCK_WRITE_MANIFEST') === '1') fs.writeFileSync(MANIFEST, serializeManifest(manifest));
    expect(manifestDrift(manifest, fs.readFileSync(MANIFEST, 'utf8'))).toEqual([]);
  });
});

describe('Pitcrew dialogs', () => {
  it('are decisions and forms that block, plus the shell’s tool and viewers', () => {
    expect([...new Set(DIALOGS.map((d) => d.kind))].sort()).toEqual(['decision', 'form', 'tool', 'viewer']);
    expect(DIALOGS.find((d) => d.id === 'pc.register')).toMatchObject({ kind: 'form', modality: 'modal' });
    expect(DIALOGS.find((d) => d.id === 'pc.confirm')).toMatchObject({ kind: 'decision', modality: 'modal' });
  });

  for (const def of DIALOGS) {
    it(`${def.id} (${def.kind}) boots with only its window's providers`, async () => {
      const report = await assertDialogBoots(def, {
        dialogs: DIALOGS,
        providers: dialogBaseProviders,
        baseCommands: DIALOG_BASE_COMMANDS,
        answers: { window_ready: { recovered: null, ui: null } },
        inputs: {
          'pc.confirm': { title: 'Remove build-pc?', body: 'It stops and is forgotten.', confirm: 'Remove', danger: true },
          'pc.stop-process': { name: 'java.exe', pid: 4242, isRunner: false, jobRunning: false },
          'pc.config-diff': { preview: { diff: { added: 1, removed: 0, hunks: [], truncated: false }, changedKeys: [], issues: [], restartNeeded: false }, maxBackups: 5 },
        },
      });
      expect(report.sent).toContain('window_ready');
    });
  }

  it('declare only commands of the tables that dialogs may call', () => {
    expect(dialogTableProblems(DIALOGS, { ...APP_COMMANDS, ...SHELL_COMMANDS }, DIALOG_BASE_COMMANDS)).toEqual([]);
  });

  it('take the base commands from the Rust table', () => {
    expect([...DIALOG_BASE]).toEqual([...DIALOG_BASE_COMMANDS].sort());
  });
});
