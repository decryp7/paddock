'use strict';
// The flat-config block for a spec folder: the rule `paddock/e2e-pinned-clock` (src/rules/e2e-pinned-clock.cjs) on `e2e/**/*.ts` except the
// wrapper `e2e/test.ts`. Every page of a spec runs on the pinned clock (`pinnedClockTest` in @decryp7/paddock-e2e); what a linter cannot say
// (a reason on every opt-out, live-page steps, a wait over an empty set) is conformance C133. `paddock/e2e-ready-reads` (an error: the lint commands run with --max-warnings 0 in every app's and the ui's existing lint step) flags a read right after a bare visibility wait (src/rules/e2e-ready-reads.cjs); the few reads that are immediate by design carry `// eslint-disable-next-line paddock/e2e-ready-reads -- <why>`, counted by conformance C139.
// Use: `module.exports = defineConfig(..., ...e2ePinnedClock())` in the app's eslint.config.js, and put `e2e/**/*.ts` in the lint target's
// lintFilePatterns (angular.json): eslint/test/wiring.test.cjs checks both for every app and the ui, and that an app's own e2e rules still fire.
const plugin = require('./plugin.cjs');

/**
 * @param {object} [options]
 * @param {string[]} [options.files]  globs of the specs (default `e2e/**\/*.ts`)
 * @param {'off'|'warn'|'error'} [options.ready]  severity of `paddock/e2e-ready-reads` (default `error`)
 * @param {string[]} [options.ignores] the wrapper, which alone may import `test` itself (default `e2e/test.ts`)
 */
function e2ePinnedClock(options = {}) {
  return [
    {
      files: options.files ?? ['e2e/**/*.ts'],
      ignores: options.ignores ?? ['e2e/test.ts'],
      plugins: { paddock: plugin },
      rules: { 'paddock/e2e-pinned-clock': 'error', 'paddock/e2e-ready-reads': options.ready ?? 'error' },
    },
  ];
}

module.exports = { e2ePinnedClock };
