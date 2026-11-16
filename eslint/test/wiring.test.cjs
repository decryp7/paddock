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
function lintPlanted(dir) {
  const script = `
    const { ESLint } = require('eslint');
    new ESLint({ cwd: ${JSON.stringify(dir)} }).lintText(${JSON.stringify(SPEC)}, { filePath: ${JSON.stringify(join(dir, PLANTED))} })
      .then((r) => process.stdout.write(JSON.stringify(r[0].messages.map((m) => m.ruleId))));`;
  const out = spawnSync(process.execPath, ['-e', script], { cwd: dir, env: { ...process.env, DEBRIEF_LINT_REPORT: '1' }, encoding: 'utf8' });
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
