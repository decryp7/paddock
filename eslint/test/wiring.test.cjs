'use strict';
// Wiring (ER-41, the class "a check wired where it never runs, or silently replaces another", 2026-11-12 and 2026-11-16): for every app and
// the ui, (1) the lint command covers the e2e folder (the lint target's lintFilePatterns in angular.json, or the ui's `eslint` script), (2) a
// planted spec linted with the app's real eslint config gets the pinned-clock rule and the ready-reads rule as an error, and (3) the app's other rules still fire in that
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
const SPEC = "/* eslint paddock/e2e-ready-reads: \"off\" */\n/* eslint-disable paddock/e2e-ready-reads */\nimport { test } from '@playwright/test';\ndebugger;\nconst raw = page.locator('[role=row].row');\nawait expect(raw).toBeVisible();\nconst box = await raw.boundingBox();\n";

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

/** The messages [{ ruleId, severity }] ESLint reports for the planted spec with the app's own config (DEBRIEF_LINT_REPORT=1 turns Debrief's report-only rule on). */
function lintPlanted(dir) {
  const script = `
    const { ESLint } = require('eslint');
    new ESLint({ cwd: ${JSON.stringify(dir)} }).lintText(${JSON.stringify(SPEC)}, { filePath: ${JSON.stringify(join(dir, PLANTED))} })
      .then((r) => process.stdout.write(JSON.stringify(r[0].messages.map((m) => ({ ruleId: m.ruleId, severity: m.severity })))));`;
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
    const messages = lintPlanted(dir);
    const rules = messages.map((m) => m.ruleId);
    assert.ok(rules.includes('paddock/e2e-pinned-clock'), `${name}: the pinned-clock rule did not fire: ${rules.join(', ')}`);
    assert.ok(rules.includes('no-debugger'), `${name}: eslint:recommended no longer applies in e2e: ${rules.join(', ')}`);
    // ER-33: the ready-reads rule must be an error in the real config (a warning or a missing block would let a new unready read through the lint step).
    const ready = messages.find((m) => m.ruleId === 'paddock/e2e-ready-reads');
    assert.ok(ready, `${name}: the ready-reads rule did not fire on a read right after a bare visibility wait: ${rules.join(', ')}`);
    assert.equal(ready.severity, 2, `${name}: paddock/e2e-ready-reads is not an error (severity ${ready.severity})`);
    if (name.endsWith('debrief/app')) assert.ok(rules.includes('no-restricted-syntax'), `${name}: the raw-row selectors were replaced: ${rules.join(', ')}`);
  });
}

// ---- exceptions: ESLint bulk suppressions (eslint-suppressions.json), the only way to except a read from paddock/e2e-ready-reads ----
const { mkdtempSync, mkdirSync, rmSync, writeFileSync } = require('node:fs');
const { tmpdir } = require('node:os');

/** Runs the ESLint CLI (as the ui's lint script does) in `dir` over its e2e folder; returns { status, out }. */
function cli(dir) {
  const bin = join(REPO, 'node_modules', '.bin', 'eslint');
  const r = spawnSync(bin, ['e2e', '--max-warnings', '0'], { cwd: dir, encoding: 'utf8' });
  return { status: r.status, out: `${r.stdout}${r.stderr}` };
}

for (const dir of targets.filter((d) => existsSync(join(d, 'eslint-suppressions.json')))) {
  const name = dir.slice(REPO.length + 1);
  test(`${name}: its eslint-suppressions.json is applied by the lint target and has no unused entry`, () => {
    const file = join(dir, 'eslint-suppressions.json');
    // The Angular builder applies suppressions only when asked; the ui's CLI finds the file in its folder by itself.
    if (existsSync(join(dir, 'angular.json')) && !name.endsWith('ui')) {
      const workspace = JSON.parse(readFileSync(join(dir, 'angular.json'), 'utf8'));
      const lintOptions = Object.values(workspace.projects).map((p) => p.architect?.lint?.options).filter(Boolean);
      assert.ok(lintOptions.length > 0, `${name}: no lint target`);
      for (const o of lintOptions) {
        assert.equal(o.applySuppressions, true, `${name}: the lint target does not apply suppressions: its exceptions would not count`);
        assert.equal(resolve(dir, o.suppressionsLocation ?? 'eslint-suppressions.json'), file, `${name}: the lint target reads another suppressions file`);
      }
    }
    // The CLI fails on a suppression that no longer occurs (the API the Angular builder uses does not), so this is where a fixed read must leave its entry.
    const r = cli(dir);
    assert.equal(r.status, 0, `${name}: eslint over e2e fails with its suppressions (an unused entry, or a new site): ${r.out.slice(0, 600)}`);
  });
}

test('positive control: a suppression that no longer occurs fails the ESLint CLI, and a used one passes', () => {
  const dir = mkdtempSync(join(tmpdir(), 'paddock-supp-'));
  try {
    mkdirSync(join(dir, 'e2e'));
    writeFileSync(join(dir, 'package.json'), '{"type":"commonjs"}');
    writeFileSync(join(dir, 'eslint.config.cjs'), "module.exports = [{ files: ['e2e/**/*.js'], rules: { 'no-debugger': 'error' } }];\n");
    writeFileSync(join(dir, 'e2e', 'a.js'), 'debugger;\n');
    writeFileSync(join(dir, 'eslint-suppressions.json'), JSON.stringify({ 'e2e/a.js': { 'no-debugger': { count: 1 } } }));
    assert.equal(cli(dir).status, 0, 'a used suppression passes');
    writeFileSync(join(dir, 'e2e', 'a.js'), 'const ok = 1;\n');
    const r = cli(dir);
    assert.notEqual(r.status, 0, `an unused suppression must fail: ${r.out}`);
    assert.match(r.out, /suppressions left that do not occur/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('nothing passes --pass-on-unpruned-suppressions (it would let a fixed read leave slack for a new one)', () => {
  const r = spawnSync('git', ['grep', '-ln', 'pass-on-unpruned-suppressions', '--', '**/package.json', 'package.json', '**/angular.json', '.gitea', 'lefthook*', 'scripts', 'tools/paddock-dev/src', 'conformance/ci'], { cwd: REPO, encoding: 'utf8' });
  assert.equal(r.stdout.trim(), '', `the flag is used in: ${r.stdout}`);
});

// A warning must block: the ui's lint script, and every app's lint target, fail on one (a directive ESLint ignores under noInlineConfig, an unused
// suppression the API reports, are warnings).
for (const dir of targets) {
  const name = dir.slice(REPO.length + 1);
  test(`${name}: the lint command fails on a warning (--max-warnings 0 or maxWarnings 0)`, () => {
    const pkg = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'));
    const script = pkg.scripts?.lint ?? '';
    if (/^eslint\s/.test(script)) {
      assert.match(script, /--max-warnings[ =]0\b/, `${name}: \`${script}\` lets a warning pass`);
      return;
    }
    const workspace = JSON.parse(readFileSync(join(dir, 'angular.json'), 'utf8'));
    const lint = Object.values(workspace.projects).map((p) => p.architect?.lint?.options).filter(Boolean);
    assert.ok(lint.length > 0, `${name}: no lint target`);
    for (const o of lint) assert.equal(o.maxWarnings, 0, `${name}: a lint target lets a warning pass (maxWarnings ${o.maxWarnings})`);
  });
}
