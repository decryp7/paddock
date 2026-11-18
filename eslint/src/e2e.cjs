'use strict';
// The flat-config block for a spec folder: the rule `paddock/e2e-pinned-clock` (src/rules/e2e-pinned-clock.cjs) on `e2e/**/*.ts` except the
// wrapper `e2e/test.ts`. Every page of a spec runs on the pinned clock (`pinnedClockTest` in @decryp7/paddock-e2e); what a linter cannot say
// (a reason on every opt-out, live-page steps, a wait over an empty set) is conformance C133. `paddock/e2e-ready-reads` (an error: the lint commands run with --max-warnings 0 in every app's and the ui's lint step, and the wiring test checks both for every real config) flags a read right after a bare visibility wait (src/rules/e2e-ready-reads.cjs). The block sets `noInlineConfig`, so no inline comment can switch a rule off; an exception to the ready-reads rule is an entry of the project's eslint-suppressions.json (user approval, ER-46), which only shrinks (conformance C139).
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
      // No inline config comment in a spec (ESLint's own grammar decides what one means; we do not parse it): a `/* eslint rule: "off" */`, a block or
      // line disable, in any spelling, is ignored and reported, so the rules of this block cannot be switched off from the spec. An exception to
      // paddock/e2e-ready-reads is an entry of the project's eslint-suppressions.json (ESLint bulk suppressions), which only shrinks (conformance C139).
      linterOptions: { noInlineConfig: true },
      plugins: { paddock: plugin },
      rules: { 'paddock/e2e-pinned-clock': 'error', 'paddock/e2e-ready-reads': options.ready ?? 'error' },
    },
  ];
}

module.exports = { e2ePinnedClock };
