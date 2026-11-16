/** jsdom gaps the app relies on. Loaded before every spec (angular.json test.setupFiles). */
import { installLeakDetector } from '@decryp7/paddock-ui/testing';
// Node reads ECharts' 688 files synchronously on first import; here that cost falls outside every test's 5 s timeout.
import './app/shared/echarts-setup';

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

// Names what a test leaves behind (live timers, fake timers, nodes in the body) against the test that left it, not the next one:
// report-only until the findings are fixed: the list is in docs/benchmarks.md (2026-11-16 leak detector); then `fail`.
installLeakDetector({ mode: 'report' });
