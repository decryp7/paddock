/** jsdom gaps the app relies on. Loaded before every spec (angular.json test.setupFiles). */
import { installLeakDetector } from '@decryp7/paddock-ui/testing';
if (!window.matchMedia) {
  window.matchMedia = (query: string) =>
    ({
      matches: false,
      media: query,
      onchange: null,
      addEventListener: () => undefined,
      removeEventListener: () => undefined,
      addListener: () => undefined,
      removeListener: () => undefined,
      dispatchEvent: () => false,
    }) as MediaQueryList;
}

// Fails the test that leaves a live timer, fake timers or a node in the body behind, instead of a later one
// (ui/projects/paddock-ui/testing/src/leaks.ts). Failing mode since the app had no findings (docs/lessons.md, 2026-11-16).
installLeakDetector({ mode: 'fail' });
