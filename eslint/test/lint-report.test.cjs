'use strict';
// `bin/lint-report.cjs` (the report step of the report-only rules, ER-41; P1-4 of the WP-0 review): it runs ESLint with
// PADDOCK_BULK_LINT=warn on the app's own config, prints every finding, writes a summary to $GITHUB_STEP_SUMMARY and exits 0 on
// findings (report-only), 2 when ESLint itself fails. Every app and the ui run it in CI (test/wiring.test.cjs checks the wiring).
const assert = require('node:assert/strict');
const { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } = require('node:fs');
const { join, resolve } = require('node:path');
const { spawnSync } = require('node:child_process');
const { test } = require('node:test');

const SCRIPT = resolve(__dirname, '..', 'bin', 'lint-report.cjs');
const PKG = resolve(__dirname, '..');

/** A throwaway app: an eslint config with the bulk block and `src/a.ts`; returns the run's status, output and summary. */
function run(source, extraEnv = {}) {
  const dir = mkdtempSync(join(resolve(__dirname, '..', '..', 'node_modules', '.cache'), 'lint-report-'));
  try {
    mkdirSync(join(dir, 'src'));
    writeFileSync(join(dir, 'src', 'a.ts'), source);
    writeFileSync(
      join(dir, 'eslint.config.cjs'),
      `const tseslint = require(${JSON.stringify(require.resolve('typescript-eslint', { paths: [PKG] }))});\n` +
        `const { bulkCalls } = require(${JSON.stringify(join(PKG, 'src', 'index.cjs'))});\n` +
        "module.exports = [{ files: ['**/*.ts'], languageOptions: { parser: tseslint.parser } }, ...bulkCalls()];\n",
    );
    const summary = join(dir, 'summary.md');
    const env = { ...process.env, GITHUB_STEP_SUMMARY: summary, ...extraEnv };
    delete env.PADDOCK_BULK_LINT;
    const out = spawnSync(process.execPath, [SCRIPT, 'src'], { cwd: dir, env, encoding: 'utf8' });
    let text = '';
    try {
      text = readFileSync(summary, 'utf8');
    } catch {
      /* no summary written */
    }
    return { status: out.status, stdout: out.stdout, stderr: out.stderr, summary: text };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test('lint-report: a per-item backend call is reported, in the log and the job summary, and the step stays green', () => {
  const r = run('for (const m of models) await backend.deleteModel(m.id);\nexport {};\n');
  assert.equal(r.status, 0, r.stderr);
  assert.match(r.stdout, /src\/a\.ts:1 .*paddock\/no-backend-call-in-loop/);
  assert.match(r.summary, /paddock\/no-backend-call-in-loop: 1/);
  assert.match(r.summary, /src\/a\.ts:1/);
});

test('lint-report positive control (ER-33): the same file without the loop reports nothing, and says so', () => {
  const r = run('await backend.deleteModels(ids);\nexport {};\n');
  assert.equal(r.status, 0, r.stderr);
  assert.match(r.summary, /no findings/);
  assert.doesNotMatch(r.stdout, /no-backend-call-in-loop/);
});

test('lint-report: ESLint failing to run is an error annotation and exit 2, never a green report', () => {
  const r = run('for (const m of models) await backend.deleteModel(m.id);\n', { PADDOCK_LINT_REPORT_CONFIG: '/nonexistent/eslint.config.cjs' });
  assert.equal(r.status, 2);
  assert.match(r.stdout + r.stderr, /::error::/);
});
