'use strict';
// The flat-config block of `paddock/no-backend-call-in-loop` (src/rules/no-backend-call-in-loop.cjs; PRINCIPLES, Bulk APIs; WP-0 G5)
// for an app's sources: `src/**/*.ts` (the ui: `projects/**/*.ts`), without specs and mock backends (a mock's loop costs no IPC).
// Use: `module.exports = defineConfig(..., ...bulkCalls())` in the app's eslint.config.js; test/wiring.test.cjs checks every app and the ui.
//
// Report-only (ER-41): the lint commands run with --max-warnings 0, so a warning would block every touched file. The rule is off
// in `npm run lint` and on in `npm run lint:report` (bin/lint-report.cjs sets PADDOCK_BULK_LINT=warn, prints the findings and writes
// the job summary without failing), which every app's CI runs (Debrief's debrief.yml step, the others' paddock.ci.json, the ui's lint
// phase). It becomes an error once a CI run of every app reports zero findings, recorded here by run id.
// First run id: pending.
const plugin = require('./plugin.cjs');

/** The severity from `PADDOCK_BULK_LINT` (`warn` or `error`), else `off`. */
function bulkFromEnv(env = process.env) {
  const v = env.PADDOCK_BULK_LINT;
  return v === 'warn' || v === 'error' ? v : 'off';
}

/**
 * @param {object} [options]
 * @param {string[]} [options.files]    globs of the sources (default `src/**\/*.ts` and `projects/**\/*.ts`)
 * @param {string[]} [options.ignores]  extra ignored globs (specs and mocks are always ignored)
 * @param {'off'|'warn'|'error'} [options.level]  severity (default: PADDOCK_BULK_LINT, else `off`)
 */
function bulkCalls(options = {}) {
  return [
    {
      files: options.files ?? ['src/**/*.ts', 'projects/**/*.ts'],
      ignores: ['**/*.spec.ts', '**/testing/**', '**/mock/**', '**/mocks/**', '**/*mock*.ts', ...(options.ignores ?? [])],
      plugins: { paddock: plugin },
      rules: { 'paddock/no-backend-call-in-loop': options.level ?? bulkFromEnv() },
    },
  ];
}

module.exports = { bulkCalls, bulkFromEnv };
