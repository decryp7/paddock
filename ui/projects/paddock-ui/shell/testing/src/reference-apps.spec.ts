import { Injector, provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import {
  buildManifest,
  CommandService,
  ContributionRegistry,
  parseManifest,
  provideContributions,
  provideShellContext,
  serializeManifest,
  SHELL_PORTS,
  type ShellNotice,
} from '@decryp7/paddock-ui/shell';
import { provideMenus, provideSubmenus } from '@decryp7/paddock-ui/shell/menu';
import { DEMO_CONTRIBUTIONS, DEMO_SUBMENUS } from '../../../../shell-demo/src/app/demo.contributions';
import { demoContext } from '../../../../shell-demo/src/app/demo-context';
import { assertContributionsValid } from './contributions-lint';

// The reference apps are the first consumers of the shell (ADR 0012 §7): their contributions pass the lint, and their
// manifests round-trip. The lazy-import lint of their registration files is in reference-sources.spec.ts.
describe('shell-demo contributions', () => {
  const notices: ShellNotice[] = [];
  beforeEach(() => {
    notices.length = 0;
    TestBed.configureTestingModule({
      providers: [
        provideZonelessChangeDetection(),
        provideContributions(DEMO_CONTRIBUTIONS),
        // The menus' kind and the demo's submenus, so the lint checks the menu bar as a whole (plan S13).
        provideMenus({ appName: 'Shell demo' }),
        provideSubmenus(...DEMO_SUBMENUS),
        provideShellContext(() => Promise.resolve(demoContext), { prefetch: 'first-use' }),
        { provide: SHELL_PORTS, useValue: { showPage: () => undefined, openDialog: () => Promise.resolve(undefined), notify: (n: ShellNotice) => void notices.push(n) } },
      ],
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
    expect(notices.map((n) => n.message)).toEqual(['Hello from the shell demo (1)', 'Greetings reset', 'Fail on purpose failed: this command always fails']);
  });

  it('give a manifest that round-trips', () => {
    const manifest = buildManifest(TestBed.inject(ContributionRegistry));
    expect(manifest.menus.find((m) => m.command === 'sd.greet')?.accelerator).toBe('CmdOrCtrl+Shift+H');
    expect(parseManifest(serializeManifest(manifest))).toEqual({ ok: true, manifest });
  });
});
