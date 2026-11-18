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

// ---- the commands that lint, read through the accessors of the files that declare them (ER-42): package.json scripts, the lint target of angular.json, the
// contract (paddock.ci.json) and the workflows. No spelling of a text search decides what a lint command is. ----
const YAML = require('yaml');
const DEFAULT_SUPPRESSIONS = 'eslint-suppressions.json';
const BAD_FLAGS = [
  [/--suppressions-location\b/, 'a non-default suppressions file (C139 and this test read only eslint-suppressions.json)'],
  [/--pass-on-unpruned-suppressions\b/, 'the flag that lets a fixed read leave slack for a new one'],
];

/** Every string of a parsed JSON value (a contract's commands), except `$comment` keys. */
function strings(value, out = []) {
  if (typeof value === 'string') out.push(value);
  else if (Array.isArray(value)) value.forEach((v) => strings(v, out));
  else if (value && typeof value === 'object') for (const [k, v] of Object.entries(value)) if (k !== '$comment') strings(v, out);
  return out;
}

/** The `run` commands of a parsed workflow. */
function workflowRuns(doc) {
  const out = [];
  for (const job of Object.values(doc?.jobs ?? {})) for (const step of job.steps ?? []) if (typeof step.run === 'string') out.push(step.run);
  return out;
}

/**
 * What is wrong with the lint commands of one project. `pkg` is its package.json, `workspace` its angular.json (null for the ui), `ci` the commands CI runs for
 * it (its contract's strings and the runs of its own workflow), `app` whether it is an app (the ui lints with the CLI already).
 */
function lintCommandProblems({ name, pkg, workspace, ci, app }) {
  const problems = [];
  const commands = [...Object.values(pkg.scripts ?? {}), ...ci];
  for (const c of commands) for (const [re, why] of BAD_FLAGS) if (re.test(c)) problems.push(`${name}: \`${c}\` carries ${why}`);
  for (const [pn, p] of Object.entries(workspace?.projects ?? {})) {
    const loc = p.architect?.lint?.options?.suppressionsLocation;
    if (loc !== undefined && loc !== DEFAULT_SUPPRESSIONS) problems.push(`${name}: lint target ${pn} reads ${loc}, not ${DEFAULT_SUPPRESSIONS}`);
  }
  if (app) {
    // The Angular builder (the ESLint API) is silent about a suppression that no longer occurs; only the CLI fails on it.
    const script = pkg.scripts?.lint ?? '';
    if (!/(?:^|&&\s*)eslint e2e --max-warnings 0\b/.test(script)) problems.push(`${name}: the lint script \`${script}\` does not run the ESLint CLI over e2e, so an unused suppression is not caught by the app's own lint step`);
    for (const c of ci.filter((x) => /\blint\b/.test(x))) {
      if (/\bng lint\b/.test(c)) problems.push(`${name}: CI runs \`${c}\`, which skips the CLI run of the lint script: run \`npm run lint\``);
    }
    if (!ci.some((c) => /^npm run lint$/.test(c.trim()))) problems.push(`${name}: no CI command runs \`npm run lint\` for the app`);
  }
  return problems;
}

/** The commands CI runs for a project: its contract's strings and the runs of the workflow named after it. */
function ciCommands(dir) {
  const name = dir.slice(REPO.length + 1);
  const app = /^apps\/([^/]+)\/app$/.exec(name)?.[1];
  const out = [];
  const contract = join(REPO, app ? `apps/${app}` : 'ui', 'paddock.ci.json');
  if (existsSync(contract)) out.push(...strings(JSON.parse(readFileSync(contract, 'utf8'))));
  const workflows = join(REPO, '.gitea', 'workflows');
  for (const f of readdirSync(workflows).filter((w) => w.endsWith('.yml') || w.endsWith('.yaml'))) {
    if (app && f.replace(/\.ya?ml$/, '') === app) out.push(...workflowRuns(YAML.parse(readFileSync(join(workflows, f), 'utf8'))));
  }
  return out;
}

/** The lint commands of every workflow, whatever app they are for (a flag anywhere is a problem). */
function allWorkflowRuns() {
  const workflows = join(REPO, '.gitea', 'workflows');
  return readdirSync(workflows).filter((w) => /\.ya?ml$/.test(w)).flatMap((f) => workflowRuns(YAML.parse(readFileSync(join(workflows, f), 'utf8'))));
}

for (const dir of targets) {
  const name = dir.slice(REPO.length + 1);
  test(`${name}: its lint commands read only eslint-suppressions.json, never pass the unpruned flag, and the app's own lint step catches an unused suppression`, () => {
    const pkg = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'));
    const workspace = existsSync(join(dir, 'angular.json')) ? JSON.parse(readFileSync(join(dir, 'angular.json'), 'utf8')) : null;
    const app = !name.endsWith('ui');
    assert.deepEqual(lintCommandProblems({ name, pkg, workspace, ci: ciCommands(dir), app }), []);
  });
}

test('no workflow command anywhere carries a flag that moves or weakens the suppressions', () => {
  for (const c of allWorkflowRuns()) for (const [re, why] of BAD_FLAGS) assert.doesNotMatch(c, re, `a workflow runs \`${c}\`: ${why}`);
});

test('positive controls: each way around the suppressions is a problem', () => {
  const good = { name: 'apps/x/app', app: true, pkg: { scripts: { lint: 'ng lint && eslint e2e --max-warnings 0' } }, workspace: { projects: { x: { architect: { lint: { options: { applySuppressions: true } } } } } }, ci: ['npm run lint'] };
  assert.deepEqual(lintCommandProblems(good), [], 'the good shape passes');
  const bad = (change) => lintCommandProblems({ ...good, ...change });
  const ws = (loc) => ({ projects: { x: { architect: { lint: { options: { applySuppressions: true, suppressionsLocation: loc } } } } } });
  assert.match(bad({ workspace: ws('lint-exceptions.json') })[0], /reads lint-exceptions\.json/);
  assert.deepEqual(bad({ workspace: ws('eslint-suppressions.json') }), [], 'the default name is fine');
  assert.match(bad({ pkg: { scripts: { lint: 'ng lint --suppressions-location x.json && eslint e2e --max-warnings 0' } } })[0], /non-default suppressions file/);
  assert.match(bad({ app: false, pkg: { scripts: { lint: 'eslint projects e2e --max-warnings 0 --suppressions-location x.json' } } })[0], /non-default suppressions file/);
  assert.match(bad({ ci: ['npm run lint', 'npx eslint e2e --pass-on-unpruned-suppressions'] })[0], /unpruned|slack/);
  assert.match(bad({ pkg: { scripts: { lint: 'ng lint' } } })[0], /does not run the ESLint CLI over e2e/);
  assert.match(bad({ ci: ['npx ng lint'] }).join('|'), /skips the CLI run/);
  assert.match(bad({ ci: [] }).join('|'), /no CI command runs `npm run lint`/);
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
