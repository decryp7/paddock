import { TestBed } from '@angular/core/testing';
import { settle } from './helpers';
import { testEnded } from './test-lifetime';

// Spec files share one page (the Angular builder runs Vitest with `isolate: false`). A test that times out is reported,
// but its body keeps running: its next `settle()` called `TestBed.tick()` on the next test's (reset) TestBed, so that
// test failed with "Cannot configure the test module when the test module has already been instantiated" (CI run 7825).
describe('settle after its test ended', () => {
  it('stops instead of touching the next test’s TestBed', async () => {
    const pending = settle(undefined, 2);
    testEnded(); // what the shared afterEach does when Vitest gives up on a timed-out test
    await expect(pending).rejects.toThrow(/outlived its test/);
    expect(() => TestBed.configureTestingModule({})).not.toThrow();
  });
});
