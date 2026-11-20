'use strict';
// The flat-config block for a spec folder: the rule `paddock/e2e-pinned-clock` (src/rules/e2e-pinned-clock.cjs) on `e2e/**/*.ts` except the
// wrapper `e2e/test.ts`. Every page of a spec runs on the pinned clock (`pinnedClockTest` in @decryp7/paddock-e2e); what a linter cannot say
// (a reason on every opt-out, live-page steps, a wait over an empty set) is conformance C133. `paddock/e2e-ready-reads` (an error: the lint commands run with --max-warnings 0 in every app's and the ui's lint step, and the wiring test checks both for every real config) flags a read right after a bare visibility wait (src/rules/e2e-ready-reads.cjs). The block sets `noInlineConfig`, so no inline comment can switch a rule off; an exception to the ready-reads rule is an entry of the project's eslint-suppressions.json (user approval, ER-46), which only shrinks (conformance C139). `paddock/e2e-transient-wait` (report-only, ER-41: off by default, `PADDOCK_E2E_TRANSIENT_LINT=warn` reads it) flags a menu or toast a spec finds itself instead of through its helper (src/rules/e2e-transient-wait.cjs).
// Use: `module.exports = defineConfig(..., ...e2ePinnedClock())` in the app's eslint.config.js, and put `e2e/**/*.ts` in the lint target's
// lintFilePatterns (angular.json): eslint/test/wiring.test.cjs checks both for every app and the ui, and that an app's own e2e rules still fire.
const plugin = require('./plugin.cjs');

/** A report-only rule's severity from its environment variable (`warn` or `error`), else `off` (ER-41). */
function severityFromEnv(name) {
  const v = process.env[name];
  return v === 'warn' || v === 'error' ? v : 'off';
}

/**
 * @param {object} [options]
 * @param {string[]} [options.files]  globs of the specs (default `e2e/**\/*.ts`)
 * @param {'off'|'warn'|'error'} [options.ready]  severity of `paddock/e2e-ready-reads` (default `error`)
 * @param {'off'|'warn'|'error'} [options.transient]  severity of `paddock/e2e-transient-wait` (default: the environment variable PADDOCK_E2E_TRANSIENT_LINT, else `off`; report-only, ER-41)
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
      rules: {
        'paddock/e2e-pinned-clock': 'error',
        'paddock/e2e-ready-reads': options.ready ?? 'error',
        'paddock/e2e-transient-wait': options.transient ?? severityFromEnv('PADDOCK_E2E_TRANSIENT_LINT'),
      },
    },
  ];
}

module.exports = { e2ePinnedClock };
