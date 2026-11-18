'use strict';
// Wiring (ER-41, the class "a check wired where it never runs, or silently replaces another", 2026-11-12 and 2026-11-16): for every app and
// the ui, (1) the lint command covers the e2e folder (the lint target's lintFilePatterns in angular.json, or the ui's `eslint` script), (2) a
// planted spec linted with the app's real eslint config gets the pinned-clock rule, and (3) the app's other rules still fire in that
// folder (Debrief's raw-row selectors, and eslint:recommended's no-debugger everywhere). The config is loaded in a child process, as `ng lint`
// does, so the environment of the test cannot change it.
const assert = require('node:assert/strict');
const { existsSync, readFileSync, readdirSync } = require('node:fs');
const { dirname, join, resolve } = require('node:path');
const { spawnSync } = require('node:child_process');
const { test } = require('node:test');
const { minimatch } = require('minimatch');

const REPO = resolve(__dirname, '..', '..');
const PLANTED = 'e2e/zz-planted.spec.ts';
const SPEC = "import { test } from '@playwright/test';\ndebugger;\nconst raw = page.locator('[role=row].row');\n";

/** The apps with an Angular workspace and an e2e folder, and the ui. */
const targets = [
  ...readdirSync(join(REPO, 'apps'))
    .map((a) => join(REPO, 'apps', a, 'app'))
    .filter((d) => existsSync(join(d, 'angular.json')) && existsSync(join(d, 'e2e'))),
  join(REPO, 'ui'),
];

/** The globs the app's lint command covers. */
function lintPatterns(dir) {
  const pkg = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'));
  const script = pkg.scripts?.lint ?? '';
  if (/^eslint\s/.test(script)) return script.replace(/^eslint\s+/, '').split(/\s+/).filter((w) => !w.startsWith('-')).map((p) => (p.includes('*') ? p : `${p}/**`));
  const workspace = JSON.parse(readFileSync(join(dir, 'angular.json'), 'utf8'));
  return Object.values(workspace.projects).flatMap((p) => p.architect?.lint?.options?.lintFilePatterns ?? []);
}

/** The rule ids ESLint reports for the planted spec with the app's own config (DEBRIEF_LINT_REPORT=1 turns Debrief's report-only rule on). */
function lintPlanted(dir, file = PLANTED, text = SPEC, env = { DEBRIEF_LINT_REPORT: '1' }) {
  const script = `
    const { ESLint } = require('eslint');
    new ESLint({ cwd: ${JSON.stringify(dir)} }).lintText(${JSON.stringify(text)}, { filePath: ${JSON.stringify(join(dir, file))} })
      .then((r) => process.stdout.write(JSON.stringify(r[0].messages.map((m) => m.ruleId))));`;
  const base = { ...process.env };
  delete base.PADDOCK_BULK_LINT;
  const out = spawnSync(process.execPath, ['-e', script], { cwd: dir, env: { ...base, ...env }, encoding: 'utf8' });
  assert.equal(out.status, 0, `${dir}: eslint failed: ${out.stderr}`);
  return JSON.parse(out.stdout);
}

for (const dir of targets) {
  const name = dir.slice(REPO.length + 1);
  test(`${name}: the lint command covers e2e, the pinned-clock rule fires there, and the app's other e2e rules still do`, () => {
    const patterns = lintPatterns(dir);
    const rel = (p) => p.replace(/^\.\//, '');
    assert.ok(
      patterns.some((p) => minimatch(PLANTED, rel(p), { dot: true })),
      `${name}: no lint pattern covers ${PLANTED} (${patterns.join(', ')}): a spec is never linted by \`npm run lint\``,
    );
    const rules = lintPlanted(dir);
    assert.ok(rules.includes('paddock/e2e-pinned-clock'), `${name}: the pinned-clock rule did not fire: ${rules.join(', ')}`);
    assert.ok(rules.includes('no-debugger'), `${name}: eslint:recommended no longer applies in e2e: ${rules.join(', ')}`);
    if (name.endsWith('debrief/app')) assert.ok(rules.includes('no-restricted-syntax'), `${name}: the raw-row selectors were replaced: ${rules.join(', ')}`);
  });
}

// `paddock/no-backend-call-in-loop` (src/bulk.cjs, WP-0 G5): every app and the ui lint their sources with it, off by default (report-only,
// ER-41) and on with PADDOCK_BULK_LINT=warn. A planted source file with a backend call in a loop is the positive control (ER-33): the
// rule must fire there when it is switched on, and stay silent when it is not.
const BULK = 'for (const m of models) await backend.deleteModel(m.id);\nexport {};\n';
const BULK_RULE = 'paddock/no-backend-call-in-loop';
for (const dir of targets) {
  const name = dir.slice(REPO.length + 1);
  const planted = name === 'ui' ? 'projects/paddock-ui/src/zz-planted-bulk.ts' : 'src/app/zz-planted-bulk.ts';
  test(`${name}: the lint command covers the sources, and ${BULK_RULE} fires there only when PADDOCK_BULK_LINT is set`, () => {
    const patterns = lintPatterns(dir).map((p) => p.replace(/^\.\//, ''));
    assert.ok(patterns.some((p) => minimatch(planted, p, { dot: true })), `${name}: no lint pattern covers ${planted} (${patterns.join(', ')})`);
    const on = lintPlanted(dir, planted, BULK, { PADDOCK_BULK_LINT: 'warn' });
    assert.ok(on.includes(BULK_RULE), `${name}: ${BULK_RULE} did not fire with PADDOCK_BULK_LINT=warn: ${on.join(', ')} (add ...bulkCalls() to its eslint config)`);
    const off = lintPlanted(dir, planted, BULK, {});
    assert.ok(!off.includes(BULK_RULE), `${name}: ${BULK_RULE} fired without PADDOCK_BULK_LINT; it is report-only until its first green run (ER-41)`);
  });
}

// The report step (ER-41, P1-4 of the WP-0 review): every app and the ui has `npm run lint:report`, which runs bin/lint-report.cjs (it sets
// PADDOCK_BULK_LINT=warn and writes the job summary), and CI runs it: a line of the app's paddock.ci.json (the ui: its `lint` phase), or
// for Debrief, whose CI is its own workflow, a `run: npm run lint:report` step in .gitea/workflows/debrief.yml.
function ciRunsReport(dir) {
  const name = dir.slice(REPO.length + 1);
  const contract = name === 'ui' ? join(REPO, 'ui', 'paddock.ci.json') : join(dir, '..', 'paddock.ci.json');
  const lines = Object.values(JSON.parse(readFileSync(contract, 'utf8'))).filter(Array.isArray).flat();
  if (lines.includes('npm run lint:report')) return true;
  const app = name.split('/')[1];
  const workflow = join(REPO, '.gitea', 'workflows', `${app}.yml`);
  return existsSync(workflow) && /^\s*run: npm run lint:report\s*$/m.test(readFileSync(workflow, 'utf8'));
}
for (const dir of targets) {
  const name = dir.slice(REPO.length + 1);
  test(`${name}: lint:report runs bin/lint-report.cjs and CI runs lint:report`, () => {
    const script = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8')).scripts?.['lint:report'] ?? '';
    assert.match(script, /eslint\/bin\/lint-report\.cjs/, `${name}: lint:report (${script || 'missing'}) does not run eslint/bin/lint-report.cjs`);
    assert.ok(ciRunsReport(dir), `${name}: no CI line runs \`npm run lint:report\` (its paddock.ci.json, or its own workflow)`);
  });
}
