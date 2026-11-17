/** jsdom gaps the app relies on. Loaded before every spec (angular.json test.setupFiles). */
import { installLeakDetector } from '@decryp7/paddock-ui/testing';
import { disposeMockBackends } from './app/core/mock-backend';
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
// Fails the test that leaves a live timer, fake timers or a node in the body behind (ui/projects/paddock-ui/testing/src/leaks.ts).
// The mock backend's own timers (live ticks, scan and job steps, first pushes) end with the test that made the mock.
afterEach(disposeMockBackends);
installLeakDetector({ mode: 'fail' });
