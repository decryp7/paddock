// `./dev ci verdict` (tools/paddock-dev/src/ci_runs/verdict.rs) is the one reader of the Actions runs for "is there a full green run
// of workflow W on commit S?". Two checks keep it that way: every workflow the tag gate can ask about declares its run scope through
// the one producer (ADR 0113: `./dev plan --ci` for ci.yml, `./dev ci scope` for the app workflows and the shared plan action),
// and no other script or tool reads the runs API to make a pass/fail verdict.
import assert from 'node:assert/strict';
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { test } from 'node:test';
import { PADDOCK } from './helpers.mjs';
import { loadWorkflow } from './lib/workflow.mjs';

// ---- the scope declaration ----------------------------------------------------------------------------------------------------
// `./dev ci verdict` trusts the line `plan scope: {json}` (run_scope::SCOPE_LINE): what the run did (`ran`) against what a push to main
// does for the same tree (`need`), both written by the planner (ADR 0113). No workflow writes "complete" with a line of its own:
// the three hand-written `plan verdict: complete=` lines (ci.yml, debrief.yml, the plan action) each forgot a dimension once (run 8335,
// the schedule-skipped and rerun-only runs of 2026-11-12, the WebKit hole of the D4 reuse). The semantics (engines, apps, jobs,
// whole tests, reruns, empty scopes) are tested in Rust, run_scope.rs; these tests keep every producer wired and the old line gone.

/** The text of one top-level job of a workflow, or null. */
export function jobText(text, id) {
  const lines = text.split('\n');
  const start = lines.findIndex((l) => l === `  ${id}:`);
  if (start < 0) return null;
  const end = lines.findIndex((l, i) => i > start && /^ {2}[\w-]+:/.test(l));
  return lines.slice(start, end < 0 ? undefined : end).join('\n');
}

/** A job's or action's code lines (comments dropped). */
export const codeLines = (body) =>
  body
    .split('\n')
    .filter((l) => !l.trim().startsWith('#'))
    .map((l) => l.trim())
    .filter(Boolean);

const planCode = (text) => codeLines(jobText(text, 'plan') ?? '');

/** Each app workflow of the tag gate, with its app: `{ 'debrief.yml': 'debrief' }`, from the apps' paddock.ci.json `workflow`. */
function appWorkflows(root) {
  const out = {};
  const apps = join(root, 'apps');
  for (const a of existsSync(apps) ? readdirSync(apps) : []) {
    const f = join(apps, a, 'paddock.ci.json');
    if (existsSync(f)) {
      const w = JSON.parse(readFileSync(f, 'utf8')).workflow;
      if (w) out[w] = a;
    }
  }
  return out;
}

const ACTION = 'actions/plan/action.yml';
const actionText = () => readFileSync(join(PADDOCK, '.gitea', ACTION), 'utf8');
/** Release workflows whose plan job is the shared plan action (it declares the scope; the workflow passes `full` and its own file). */
const ACTION_WORKFLOWS = { 'dyno-release.yml': 'dyno', 'pitcrew-release.yml': 'pitcrew', 'pitwall-release.yml': 'pitwall', 'spotter-release.yml': 'spotter' };

const text = (w) => readFileSync(join(PADDOCK, '.gitea', 'workflows', w), 'utf8');

const RUN_OUTPUT = 'echo "run=$run" >> "$GITHUB_OUTPUT"';
const OLD_LINE = /plan verdict:|complete=(?:true|false|\$complete)/;

/** Why a ci.yml plan job does not declare its scope through the planner, or null: it runs `./dev plan --ci` and writes no verdict. */
export function ciProblem(workflowText, code = planCode(workflowText)) {
  if (code.length === 0) return 'it has no `plan` job';
  if (!code.includes('./dev plan --ci')) return 'its plan job does not run `./dev plan --ci`, which prints the `plan scope:` line';
  const old = code.find((l) => OLD_LINE.test(l));
  if (old) return `its plan job writes a verdict of its own (${old}); the planner declares the scope and the reader decides`;
  return null;
}

/**
 * Why a plan job (or action) that decides `run` itself does not declare its scope through `./dev ci scope`, or null: the call comes
 * after `run` is written, names this workflow and app, passes the run flag and the full mode (and the rerun input when the job has
 * one), and no verdict line of the old kind is left.
 */
export function scopeProblem(code, { file, app, workflowArg = file, full = '"$PLAN_MODE_FULL"', sinceRun = false, runOutput = RUN_OUTPUT }) {
  if (code.length === 0) return 'it has no plan code';
  const at = code.findIndex((l) => l.startsWith('./dev ci scope ') || l.includes(' ci scope '));
  if (at < 0) return 'it does not run `./dev ci scope`, which prints the `plan scope:` line';
  const call = code[at];
  const old = code.find((l) => OLD_LINE.test(l));
  if (old) return `it still writes a verdict of its own (${old})`;
  const wrote = code.findIndex((l) => l === runOutput);
  if (wrote < 0) return `it does not write \`${runOutput}\`, the decision the scope depends on`;
  if (at < wrote) return 'it declares the scope before it has finished deciding `run`';
  const has = (flag) => call.includes(flag);
  if (!has(`--workflow ${workflowArg}`)) return `its \`ci scope\` call does not name the workflow (${workflowArg})`;
  if (!has(`--app ${app}`)) return `its \`ci scope\` call does not name the app (${app})`;
  if (!has('--run "$run"')) return 'its `ci scope` call does not pass `--run "$run"`';
  if (!has(`--full ${full}`)) return `its \`ci scope\` call does not pass \`--full ${full}\``;
  if (sinceRun && !has('--since-run "$PLAN_SINCE_RUN"')) return 'its `ci scope` call does not pass `--since-run "$PLAN_SINCE_RUN"`';
  return null;
}

test('ci.yml declares its scope through the planner and writes no verdict line of its own', () => {
  assert.equal(ciProblem(text('ci.yml')), null);
  // The planner is the one producer: `./dev plan --ci` prints the line (plan_cmd.rs), which a conformance test cannot see, so the
  // wiring is that the plan step runs it and that nothing else in the job prints a verdict.
  assert.match(jobText(text('ci.yml'), 'plan'), /id: plan[\s\S]*run: \|[\s\S]*\.\/dev plan --ci/);
});

test('every app workflow of the tag gate declares its scope through `./dev ci scope` after it decided run', () => {
  const apps = appWorkflows(PADDOCK);
  assert.ok(apps['debrief.yml'] === 'debrief', Object.keys(apps).join(', '));
  for (const [file, app] of Object.entries(apps)) {
    if (file in ACTION_WORKFLOWS) continue; // through the shared plan action, below
    const sinceRun = planCode(text(file)).some((l) => l.includes('PLAN_SINCE_RUN'));
    assert.equal(scopeProblem(planCode(text(file)), { file, app, sinceRun }), null, file);
  }
  assert.equal(scopeProblem(planCode(text('debrief.yml')), { file: 'debrief.yml', app: 'debrief', sinceRun: true }), null);
});

test('the shared plan action declares the scope, and its release callers pass their own file and the full mode', () => {
  const action = codeLines(actionText());
  assert.equal(scopeProblem(action, { file: 'x', app: '"$APP"', workflowArg: '"$WORKFLOW"', full: '"$PLAN_MODE_FULL"', runOutput: 'echo "run=$run" >> "$GITHUB_OUTPUT"' }), null);
  assert.ok(action.includes('PLAN_MODE_FULL: ${{ inputs.full }}'), 'the action takes the full mode from its `full` input');
  assert.ok(action.includes('WORKFLOW: ${{ inputs.workflow }}'), 'the action takes the workflow file from its `workflow` input');
  assert.ok(!action.some((l) => l.includes('PLAN_SINCE_RUN')), 'the action has no since-run input (the workflows that rerun do not use it)');
  for (const [w, app] of Object.entries(ACTION_WORKFLOWS)) {
    const plan = jobText(text(w), 'plan') ?? '';
    assert.match(plan, /uses: \.\/\.gitea\/actions\/plan/, `${w}: its plan job uses the plan action`);
    assert.match(plan, new RegExp(`^\\s+workflow: ${w.replace('.', '\\.')}$`, 'm'), `${w}: it passes its own file as \`workflow:\``);
    assert.match(plan, new RegExp(`^\\s+app: ${app}$`, 'm'), `${w}: it passes its app`);
    assert.match(plan, /^\s+full: \S/m, `${w}: it passes \`full:\` (the scope of the run)`);
    assert.doesNotMatch(plan, OLD_LINE, `${w}: no hand-written verdict line`);
    assert.equal(loadWorkflow(w).jobs.plan.steps.find((s) => s.uses === './.gitea/actions/plan')?.with?.workflow, w, `${w}: the declared file is the workflow's own`);
  }
});

test('the producer checks see a missing call, an early call, another workflow or app, a lost flag and an old verdict line (positive control)', () => {
  const ok = [
    'run=$(./dev plan --ci --app x)',
    'echo "run=$run" >> "$GITHUB_OUTPUT"',
    './dev ci scope --workflow x.yml --app x --full "$PLAN_MODE_FULL" --run "$run" --since-run "$PLAN_SINCE_RUN" --full-engines chromium,webkit',
  ];
  const opts = { file: 'x.yml', app: 'x', sinceRun: true };
  assert.equal(scopeProblem(ok, opts), null);
  assert.match(scopeProblem(ok.slice(0, 2), opts), /does not run `.\/dev ci scope`/);
  assert.match(scopeProblem([ok[2], ok[0], ok[1]], opts), /before it has finished deciding/);
  assert.match(scopeProblem([ok[0], ok[2]], opts), /does not write/);
  assert.match(scopeProblem(ok.map((l) => l.replace('--workflow x.yml', '--workflow y.yml')), opts), /does not name the workflow/);
  assert.match(scopeProblem(ok.map((l) => l.replace('--app x', '--app y')), opts), /does not name the app/);
  assert.match(scopeProblem(ok.map((l) => l.replace(' --run "$run"', '')), opts), /--run "\$run"/);
  assert.match(scopeProblem(ok.map((l) => l.replace(' --full "$PLAN_MODE_FULL"', '')), opts), /--full/);
  assert.match(scopeProblem(ok.map((l) => l.replace(' --since-run "$PLAN_SINCE_RUN"', '')), opts), /--since-run/);
  assert.match(scopeProblem([...ok, 'echo "plan verdict: complete=$complete"'], opts), /still writes a verdict/);
  // ci.yml: the planner call is needed, and a hand-written verdict is refused.
  const ci = ['./dev plan --ci', 'grep -q x'];
  assert.equal(ciProblem(null, ci), null);
  assert.match(ciProblem(null, ['grep -q x']), /does not run/);
  assert.match(ciProblem(null, [...ci, 'echo "plan verdict: complete=true"']), /verdict of its own/);
  assert.match(ciProblem(null, [...ci, 'complete=false']), /verdict of its own/);
  assert.match(ciProblem('jobs:\n  other:\n    x: 1\n'), /no `plan` job/);
});

// ---- no second reader ---------------------------------------------------------------------------------------------------------
/** What reads the Actions runs: the API paths, the run list's JSON key, and the Rust client types. */
export const READER = /actions\/(?:runs|jobs)\b|\bworkflow_runs\b|GiteaRuns|ci_runs::client|\.runs_page|\/runs\?|\/runs\/\$\{/;

/**
 * A file that reads the runs API for a purpose that is not "is there a full green run on this commit" says so itself, on a line of
 * its own: `// ci-runs-reader: <reason>` (`#` in YAML and shell), the reason at least 10 characters. A marker lives with the code
 * it excuses, so a merge of two branches that each add one never conflicts (the central allow-list this replaces went red on
 * three merges).
 */
export const MARKER = /^\s*(?:\/\/|#)\s*ci-runs-reader:\s*(\S.{9,})$/m;

const SKIP_DIR = new Set(['node_modules', 'target', '.git', 'tests', 'fixtures', 'dist']);
const SOURCE = /\.(?:mjs|js|sh|rs|py|ts|yml|yaml)$/;
const TEST_FILE = /\.test\.(?:mjs|js|ts|sh)$/;

/** Every source file under `dirs` of `root`: [relative path, text]. */
function sources(root, dirs) {
  const out = [];
  const walk = (dir) => {
    for (const name of readdirSync(dir)) {
      const p = join(dir, name);
      if (statSync(p).isDirectory()) {
        if (!SKIP_DIR.has(name)) walk(p);
      } else if (SOURCE.test(name) && !TEST_FILE.test(name)) {
        out.push([relative(root, p).split('\\').join('/'), readFileSync(p, 'utf8')]);
      }
    }
  };
  for (const d of dirs) if (existsSync(join(root, d))) walk(join(root, d));
  return out;
}

const DIRS = ['conformance/ci', 'conformance/src', 'scripts', 'tools', '.gitea'];

/** Files that carry a `ci-runs-reader:` marker but no longer read the runs API: the excuse outlived the reason. */
export function staleMarkers(root, dirs = DIRS) {
  return sources(root, dirs)
    .filter(([, text]) => MARKER.test(text) && !READER.test(text))
    .map(([rel]) => rel)
    .sort();
}

/** The source files under `dirs` of `root` whose text reads the runs API and are not in `allowed`. */
export function secondReaders(root, dirs = ['conformance/ci', 'conformance/src', 'scripts', 'tools', '.gitea']) {
  const found = [];
  const walk = (dir) => {
    for (const name of readdirSync(dir)) {
      const p = join(dir, name);
      if (statSync(p).isDirectory()) {
        if (!SKIP_DIR.has(name)) walk(p);
      } else if (SOURCE.test(name) && !TEST_FILE.test(name)) {
        const rel = relative(root, p).split('\\').join('/');
        const text = readFileSync(p, 'utf8');
        if (READER.test(text) && !MARKER.test(text)) found.push(rel);
      }
    }
  };
  for (const d of dirs) if (existsSync(join(root, d))) walk(join(root, d));
  return found.sort();
}

test('no script or tool reads the Actions runs API for a pass/fail verdict except ./dev ci verdict (a `ci-runs-reader:` marker with a reason)', () => {
  assert.deepEqual(
    secondReaders(PADDOCK),
    [],
    'a second reader of the runs: ask `./dev ci verdict --workflow <file> --sha <sha>` instead (exit 0 full green, 5 red or partial, 6 not measured); if it reads runs for another purpose, put a `// ci-runs-reader: <reason>` line (`#` in YAML) in the file',
  );
});

test('the scan sees a second reader (positive control)', async () => {
  const { makeApp } = await import('./helpers.mjs');
  const root = makeApp({
    'scripts/bad.mjs': 'const r = await fetch(`${api}/repos/o/r/actions/runs?head_sha=${sha}`);\n',
    'scripts/rust-bad.rs': 'use crate::ci_runs::client::Client;\n',
    'scripts/good.mjs': 'console.log("nothing");\n',
    'scripts/bad.test.mjs': 'actions/runs\n',
    'scripts/marked.mjs': '// ci-runs-reader: downloads the artifacts of one run\nactions/runs\n',
    'scripts/marked.yml': '# ci-runs-reader: names the runs API in a comment\nactions/runs\n',
    'scripts/short.mjs': '// ci-runs-reader: x\nactions/runs\n',
    'scripts/inline.mjs': 'const x = 1; // ci-runs-reader: not on its own line\nactions/runs\n',
    'scripts/ident.rs': 'let commit_workflow_runs = 1;\n',
  });
  assert.deepEqual(secondReaders(root, ['scripts']), ['scripts/bad.mjs', 'scripts/inline.mjs', 'scripts/rust-bad.rs', 'scripts/short.mjs']);
});

test('no marker outlives the read it excuses (a marker in a file that no longer reads the runs is an error)', () => {
  assert.deepEqual(staleMarkers(PADDOCK), [], 'remove the `ci-runs-reader:` line from these files: they no longer read the Actions runs API');
});

test('the stale-marker scan sees a marker without a read (positive control)', async () => {
  const { makeApp } = await import('./helpers.mjs');
  const root = makeApp({
    'scripts/stale.mjs': '// ci-runs-reader: used to download the artifacts of a run\nconsole.log(1);\n',
    'scripts/live.mjs': '// ci-runs-reader: downloads the artifacts of one run\nactions/runs\n',
    'scripts/none.mjs': 'console.log(2);\n',
  });
  assert.deepEqual(staleMarkers(root, ['scripts']), ['scripts/stale.mjs']);
});
