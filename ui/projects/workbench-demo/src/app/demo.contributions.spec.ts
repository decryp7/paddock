import { Injector } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { assertContributionsValid } from '@decryp7/paddock-ui/shell/testing';
import { ASSISTANT_CONTRIBUTIONS } from '@decryp7/paddock-ui/workbench/assistant';
import { providePalette } from '@decryp7/paddock-ui/workbench/palette';
import { provideWorkbenchTesting } from '@decryp7/paddock-ui/workbench/testing';
import { DEMO_CONTRIBUTIONS, DEMO_VIEWS } from './demo.contributions';

// The app's contributions, with the workbench parts its main window registers (the palette, the assistant), pass the
// registry's lint (ids, kinds, commands, keys, views and their layouts): a broken contribution fails here instead of when a
// user opens the window.
describe('the app contributions', () => {
  it('pass the contribution lint', () => {
    TestBed.configureTestingModule({ providers: provideWorkbenchTesting({ contributions: [DEMO_CONTRIBUTIONS, ASSISTANT_CONTRIBUTIONS], providers: [providePalette()] }) });
    expect(() => assertContributionsValid(TestBed.inject(Injector))).not.toThrow();
  });

  it('declare a layout for every view', () => {
    expect(DEMO_VIEWS.length).toBeGreaterThan(0);
    for (const view of DEMO_VIEWS) expect(view.layout, view.id).toBeDefined();
  });
});
