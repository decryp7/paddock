'use strict';
// The flat-config block for a spec folder: the rule `paddock/e2e-pinned-clock` (src/rules/e2e-pinned-clock.cjs) on `e2e/**/*.ts` except the
// wrapper `e2e/test.ts`. Every page of a spec runs on the pinned clock (`pinnedClockTest` in @decryp7/paddock-e2e); what a linter cannot say
// (a reason on every opt-out, live-page steps, a wait over an empty set) is conformance C133. `paddock/e2e-ready-reads` (off by default, ER-41: report-only until its first green run on the real tree; `PADDOCK_E2E_READY_LINT=warn` reads it) flags a read right after a bare visibility wait (src/rules/e2e-ready-reads.cjs).
// Use: `module.exports = defineConfig(..., ...e2ePinnedClock())` in the app's eslint.config.js, and put `e2e/**/*.ts` in the lint target's
// lintFilePatterns (angular.json): eslint/test/wiring.test.cjs checks both for every app and the ui, and that an app's own e2e rules still fire.
const plugin = require('./plugin.cjs');

/** Report-only (ER-41): the lint commands run with --max-warnings 0, so a warning would block every touched spec; the rule is off until its first green run and is read with `PADDOCK_E2E_READY_LINT=warn npx eslint 'e2e/**\/*.ts'`. */
function readyFromEnv() {
  const v = process.env.PADDOCK_E2E_READY_LINT;
  return v === 'warn' || v === 'error' ? v : 'off';
}

/**
 * @param {object} [options]
 * @param {string[]} [options.files]  globs of the specs (default `e2e/**\/*.ts`)
 * @param {'off'|'warn'|'error'} [options.ready]  severity of `paddock/e2e-ready-reads` (default: the environment variable PADDOCK_E2E_READY_LINT, else `off`)
 * @param {string[]} [options.ignores] the wrapper, which alone may import `test` itself (default `e2e/test.ts`)
 */
function e2ePinnedClock(options = {}) {
  return [
    {
      files: options.files ?? ['e2e/**/*.ts'],
      ignores: options.ignores ?? ['e2e/test.ts'],
      plugins: { paddock: plugin },
      rules: { 'paddock/e2e-pinned-clock': 'error', 'paddock/e2e-ready-reads': options.ready ?? readyFromEnv() },
    },
  ];
}

module.exports = { e2ePinnedClock };
