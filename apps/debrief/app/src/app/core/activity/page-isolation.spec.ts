import { LeakDetector } from '@decryp7/paddock-ui/testing';
import { activeViewTitle } from './runtime';

// Spec files share one page (the Angular builder runs Vitest with `isolate: false`). Elements a spec left in the body
// (a dock with an active tab, for example) were read by the next spec: an uncaught error was logged with
// `View: "Inspector"` in the logger spec, depending on which files a worker ran together. The leak detector now names a test
// that leaves such an element and removes it (fail mode); test-setup.ts also empties the body before every test. Both halves
// are shown here within one test, so no test leaves a node on purpose.
describe('the page between tests', () => {
  it('a dock tab a test leaves in the page is read by the activity runtime, named by the detector and removed', () => {
    const detector = new LeakDetector();
    detector.begin();
    const tab = document.createElement('div');
    tab.className = 'dv-tab dv-active-tab';
    tab.textContent = 'Inspector';
    document.body.appendChild(tab);
    expect(activeViewTitle()).toBe('Inspector');
    detector.snapshot(vi);
    const found = detector.finish(true);
    expect(found.map((f) => [f.kind, f.message])).toContainEqual(['dom', 'left <div.dv-tab.dv-active-tab> in document.body']);
    expect(tab.isConnected).toBe(false);
    expect(activeViewTitle()).toBeUndefined();
  });

  it('the page starts empty, as test-setup.ts leaves it before every test', () => {
    expect(document.body.childElementCount).toBe(0);
    expect(activeViewTitle()).toBeUndefined();
  });
});
