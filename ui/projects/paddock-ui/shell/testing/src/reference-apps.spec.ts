import { Injector } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import {
  AppearanceStore,
  BrowserAppearanceBackend,
  buildManifest,
  CommandService,
  ContributionRegistry,
  parseManifest,
  provideAppearance,
  manifestDrift,
  serializeManifest,
  themeProblems,
} from '@decryp7/paddock-ui/shell';
import { provideMenus, provideSubmenus } from '@decryp7/paddock-ui/shell/menu';
import { SETTINGS_CONTRIBUTIONS } from '@decryp7/paddock-ui/shell/settings';
import { OS_NOTIFICATIONS_CONTRIBUTIONS } from '@decryp7/paddock-ui/shell/notify';
import { SETUP_CONTRIBUTIONS } from '@decryp7/paddock-ui/shell/setup';
import { DEMO_CONTRIBUTIONS, DEMO_SUBMENUS, SHELL_ABOUT_CONTRIBUTIONS, SHELL_DIAGNOSTICS_CONTRIBUTIONS } from '../../../../shell-demo/src/app/demo.contributions';
import { demoContext } from '../../../../shell-demo/src/app/demo-context';
import { WARM_PAPER } from '../../../../shell-demo/src/app/demo-appearance';
import { assertContributionsValid } from './contributions-lint';
import { provideShellTesting, shellPorts } from './shell-testing';

// The reference apps are the first consumers of the shell (ADR 0012 §7): their contributions pass the lint, and their
// manifests round-trip. The lazy-import lint of their registration files is in reference-sources.spec.ts.
describe('shell-demo contributions', () => {
  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: provideShellTesting({
        contributions: [DEMO_CONTRIBUTIONS],
        context: () => Promise.resolve(demoContext),
        // The menus' kind and the demo's submenus, so the lint checks the menu bar as a whole (plan S13).
        providers: [
          provideMenus({ appName: 'Shell demo' }),
          provideSubmenus(...DEMO_SUBMENUS),
          // The demo context reads the appearance (plan S15); no storage or channel in a spec.
          provideAppearance({ accent: 'pitwall', backend: () => new BrowserAppearanceBackend({ storage: null, channel: null }) }),
        ],
      }),
    });
  });

  it('pass assertContributionsValid', () => {
    expect(() => assertContributionsValid(TestBed.inject(Injector))).not.toThrow();
  });

  it('run against the demo context', async () => {
    const commands = TestBed.inject(CommandService);
    await expect(commands.run('sd.reset')).resolves.toBe(false); // disabled: nothing to reset
    await expect(commands.run('sd.greet')).resolves.toBe(true);
    await expect(commands.run('sd.reset')).resolves.toBe(true);
    await expect(commands.run('sd.fail')).resolves.toBe(false);
    expect(shellPorts().messages()).toEqual(['Hello from the shell demo (1)', 'Greetings reset', 'Fail on purpose failed: this command always fails']);
  });

  it('switch the appearance from the View > Appearance commands (plan S15)', async () => {
    const commands = TestBed.inject(CommandService);
    const appearance = TestBed.inject(AppearanceStore);
    await expect(commands.run('sd.theme.dark')).resolves.toBe(true);
    expect(appearance.theme()).toBe('dark');
    await commands.run('sd.contrast');
    expect(appearance.resolved().highContrast).toBe(true);
    await commands.run('sd.motion');
    expect(appearance.resolved().reducedMotion).toBe(true);
    await commands.run('sd.contrast');
    await commands.run('sd.motion');
    expect(appearance.contrast()).toBe('standard');
    expect(appearance.motion()).toBe('full');
  });

  it('ship a theme pack that passes the token checks', () => {
    expect(themeProblems(WARM_PAPER)).toEqual([]);
  });

  it('give a manifest that round-trips', () => {
    const manifest = buildManifest(TestBed.inject(ContributionRegistry));
    expect(manifest.menus.find((m) => m.command === 'sd.greet')?.accelerator).toBe('CmdOrCtrl+Shift+H');
    expect(parseManifest(serializeManifest(manifest))).toEqual({ ok: true, manifest });
  });
});

/** Node's `fs`, loaded at run time (the spec programme has no Node types). */
interface NodeFs {
  readFileSync(path: string, encoding: 'utf8'): string;
  writeFileSync(path: string, data: string): void;
}
const nodeFs = (): Promise<NodeFs> => import(/* @vite-ignore */ ['node', 'fs'].join(':')) as Promise<NodeFs>;
const env = (name: string): string | undefined => (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env[name];
/** Rust's copy of the main window's contributions (`paddock_shell::manifest::check` reads it in `crates/paddock-shell/tests/it/shell_demo.rs`). */
const MANIFEST = 'projects/shell-demo/paddock-manifest.json';

describe('shell-demo manifest', () => {
  // Regenerate with PADDOCK_WRITE_MANIFEST=1 npx ng test paddock-ui --watch=false --include=**/reference-apps.spec.ts
  it('keeps paddock-manifest.json equal to the main window’s contributions', async () => {
    TestBed.configureTestingModule({
      providers: provideShellTesting({
        contributions: [DEMO_CONTRIBUTIONS, SHELL_ABOUT_CONTRIBUTIONS, SHELL_DIAGNOSTICS_CONTRIBUTIONS, SETUP_CONTRIBUTIONS, OS_NOTIFICATIONS_CONTRIBUTIONS, SETTINGS_CONTRIBUTIONS],
        context: () => Promise.resolve(demoContext),
        providers: [provideMenus({ appName: 'Shell demo' }), provideSubmenus(...DEMO_SUBMENUS)],
      }),
    });
    const manifest = buildManifest(TestBed.inject(ContributionRegistry));
    expect(manifest.settingsForms).toEqual(['pd.appearance', 'pd.logging', 'pd.notifications', 'sd.general', 'sd.storage']);
    const fs = await nodeFs();
    if (env('PADDOCK_WRITE_MANIFEST') === '1') fs.writeFileSync(MANIFEST, serializeManifest(manifest));
    expect(manifestDrift(manifest, fs.readFileSync(MANIFEST, 'utf8'))).toEqual([]);
  });
});
