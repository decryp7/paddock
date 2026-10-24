import { Injector } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { buildManifest, CommandService, ContributionRegistry, parseManifest, serializeManifest } from '@decryp7/paddock-ui/shell';
import { provideMenus, provideSubmenus } from '@decryp7/paddock-ui/shell/menu';
import { DEMO_CONTRIBUTIONS, DEMO_SUBMENUS } from '../../../../shell-demo/src/app/demo.contributions';
import { demoContext } from '../../../../shell-demo/src/app/demo-context';
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
        providers: [provideMenus({ appName: 'Shell demo' }), provideSubmenus(...DEMO_SUBMENUS)],
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

  it('give a manifest that round-trips', () => {
    const manifest = buildManifest(TestBed.inject(ContributionRegistry));
    expect(manifest.menus.find((m) => m.command === 'sd.greet')?.accelerator).toBe('CmdOrCtrl+Shift+H');
    expect(parseManifest(serializeManifest(manifest))).toEqual({ ok: true, manifest });
  });
});
