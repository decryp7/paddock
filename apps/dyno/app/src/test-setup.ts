/** jsdom gaps the app relies on. Loaded before every spec (angular.json test.setupFiles). */
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
