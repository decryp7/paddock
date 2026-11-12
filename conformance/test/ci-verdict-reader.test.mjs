// `./dev ci verdict` (tools/paddock-dev/src/ci_runs/verdict.rs) is the one reader of the Actions runs for "is there a full green run
// of workflow W on commit S?". Two checks keep it that way: every workflow the tag gate can ask about prints the full-mode marker
// in its plan job, and no other script or tool reads the runs API to make a pass/fail verdict.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { test } from 'node:test';
import { PADDOCK } from './helpers.mjs';

// ---- the marker ---------------------------------------------------------------------------------------------------------------
// `./dev ci verdict` trusts the line `plan verdict: complete=true` (plan_since::COMPLETE_LINE), which the plan job prints AFTER its
// decision: the plan ran its jobs, the run is not a rerun of failures (since_run empty) and the mode is full. The scope line
// `plan mode: full=` (plan_since::MODE_LINE) is logged before the decision and proves nothing about it.

/** The text of one top-level job of a workflow, or null. */
export function jobText(text, id) {
  const lines = text.split('\n');
  const start = lines.findIndex((l) => l === `  ${id}:`);
  if (start < 0) return null;
  const end = lines.findIndex((l, i) => i > start && /^ {2}[\w-]+:/.test(l));
  return lines.slice(start, end < 0 ? undefined : end).join('\n');
}

const VERDICT_ECHO = 'echo "plan verdict: complete=$complete"';

/** The plan job's code lines (comments dropped). */
const planCode = (text) =>
  (jobText(text, 'plan') ?? '')
    .split('\n')
    .filter((l) => !l.trim().startsWith('#'))
    .map((l) => l.trim());

/** Why a workflow's plan job does not print the verdict marker after its decision, or null when it does. */
export function markerProblem(text) {
  if (jobText(text, 'plan') == null) return 'it has no `plan` job';
  const code = planCode(text);
  const at = code.indexOf(VERDICT_ECHO);
  if (at < 0) return `its plan job does not \`${VERDICT_ECHO}\` (plan_since::COMPLETE_LINE)`;
  const decided = code.findLastIndex((l) => /^(?:\.\/dev plan\b|run=\$\(|echo "run=|node scripts\/affected\.mjs|grep -q|if \[ "\$EVENT")/.test(l) || /^echo "(?:run|crate_legs|paddock_dev_legs)=/.test(l) || /\|\| echo "(?:crate_legs|paddock_dev_legs)=/.test(l));
  if (decided > at) return 'its plan job prints the verdict line before it has finished deciding';
  if (!code.some((l) => /^if \[.*-z "\$PLAN_SINCE_RUN".*PLAN_MODE_FULL" = true.*complete=true/.test(l))) return 'its complete=true condition does not need an empty PLAN_SINCE_RUN and PLAN_MODE_FULL = true';
  return null;
}

/** The three lines that decide `complete` (from `complete=false` to the echo) of a plan job. */
export function verdictSnippet(text) {
  const code = planCode(text);
  const at = code.indexOf(VERDICT_ECHO);
  const from = code.lastIndexOf('complete=false', at);
  return from < 0 || at < 0 ? null : code.slice(from, at + 1).join('\n');
}

/** Runs the snippet in bash with the plan's variables; returns the line it prints. */
function runSnippet(snippet, env) {
  return spawnSync('bash', ['-c', snippet], { encoding: 'utf8', env: { PATH: process.env.PATH, ...env } }).stdout.trim();
}

/** The workflows the tag gate can ask about: ci.yml (an app's legs, the demos, the shared packages) and each app's own `workflow`. */
function gateWorkflows(root) {
  const out = new Set(['ci.yml']);
  const apps = join(root, 'apps');
  for (const a of existsSync(apps) ? readdirSync(apps) : []) {
    const f = join(apps, a, 'paddock.ci.json');
    if (existsSync(f)) {
      const w = JSON.parse(readFileSync(f, 'utf8')).workflow;
      if (w) out.add(w);
    }
  }
  return [...out].sort();
}

const text = (w) => readFileSync(join(PADDOCK, '.gitea', 'workflows', w), 'utf8');

test('every gate workflow prints the verdict marker after its plan decided (the tag gate reads it through ./dev ci verdict)', () => {
  const wfs = gateWorkflows(PADDOCK);
  assert.ok(wfs.includes('ci.yml') && wfs.includes('debrief.yml'), wfs.join(', '));
  for (const w of wfs) {
    const p = markerProblem(text(w));
    assert.equal(p, null, `${w}: ${p}. Without the marker a dispatch run is never full, so the tag workflow refuses the app's tag as "not full".`);
  }
});

test('the verdict is complete=true only for a plan that ran, a full mode and no since_run (the snippets, run in bash)', () => {
  for (const w of gateWorkflows(PADDOCK)) {
    const snippet = verdictSnippet(text(w));
    assert.ok(snippet, `${w}: no verdict snippet`);
    const decides = snippet.includes('"$run"');
    const say = (env) => runSnippet(snippet, { run: 'true', PLAN_MODE_FULL: 'true', PLAN_SINCE_RUN: '', ...env });
    assert.equal(say({}), 'plan verdict: complete=true', w);
    // A since_run rerun (debrief: with bundles, so the scope line says full=true) is not complete.
    assert.equal(say({ PLAN_SINCE_RUN: '8001' }), 'plan verdict: complete=false', `${w}: since_run`);
    assert.equal(say({ PLAN_MODE_FULL: 'false' }), 'plan verdict: complete=false', `${w}: not full`);
    // A run the plan skipped (a schedule skipped for a train, an unchanged main, a tagged commit) ends green having tested nothing.
    if (decides) assert.equal(say({ run: 'false' }), 'plan verdict: complete=false', `${w}: schedule-skipped (run=false)`);
  }
  assert.ok(verdictSnippet(text('debrief.yml')).includes('"$run"'), 'debrief.yml decides run=');
});

test('the marker check sees a missing, early or unguarded marker (positive control)', () => {
  const ok = [
    'jobs:',
    '  plan:',
    '    steps:',
    '      - run: |',
    '          run=$(./dev plan --ci --app x)',
    '          echo "run=$run" >> "$GITHUB_OUTPUT"',
    '          complete=false',
    '          if [ "$run" = true ] && [ -z "$PLAN_SINCE_RUN" ] && [ "$PLAN_MODE_FULL" = true ]; then complete=true; fi',
    '          echo "plan verdict: complete=$complete"',
    '  next:',
    '    x: 1',
    '',
  ].join('\n');
  assert.equal(markerProblem(ok), null);
  assert.match(markerProblem(ok.replace('echo "plan verdict', '# echo "plan verdict')), /does not/);
  assert.match(markerProblem(ok.replace('-z "$PLAN_SINCE_RUN" ] && [ ', '')), /does not need an empty PLAN_SINCE_RUN/);
  assert.match(markerProblem(ok.replace('          echo "plan verdict: complete=$complete"\n', '').replace('run=$(./dev plan', 'echo "plan verdict: complete=$complete"\n          run=$(./dev plan')), /before it has finished/);
  assert.match(markerProblem('jobs:\n  other:\n    x: 1\n'), /no `plan` job/);
  // The scope line alone is not the marker.
  assert.match(markerProblem(ok.replace('plan verdict: complete=', 'plan mode: full=')), /does not/);
});

// ---- no second reader ---------------------------------------------------------------------------------------------------------
/** What reads the Actions runs: the API paths, the run list's JSON key, and the Rust client types. */
export const READER = /actions\/(?:runs|jobs)\b|workflow_runs|GiteaRuns|ci_runs::client|\.runs_page|\/runs\?|\/runs\/\$\{/;

/** Files that read the runs API for a purpose that is not "is there a full green run on this commit", each with the reason. */
export const ALLOWED = {
  'tools/paddock-dev/src/ci_runs/client.rs': 'the shared Gitea client that ./dev ci verdict, flakes, timings and schedule-skip use',
  'tools/paddock-dev/src/ci_runs/verdict.rs': 'the verdict reader itself',
  'tools/paddock-dev/src/ci_runs/flakes.rs': 'ci flakes: flaky tests over a window of runs',
  'tools/paddock-dev/src/ci_runs/timings.rs': 'ci timings: durations of runs and jobs',
  'tools/paddock-dev/src/ci_runs/schedule_skip.rs': 'ci schedule-skip: is a train in CI (queued or running dispatch runs)',
  'tools/paddock-dev/src/ci_runs/testutil.rs': 'the fake Gitea of the tests',
  'tools/paddock-dev/src/ci_runs/mod.rs': 'the commands over the client: flakes, timings, schedule-skip, verdict',
  'tools/paddock-dev/src/wait.rs': 'wait --ci-run: waits for one run to finish; no pass/fail on a commit',
  'tools/paddock-dev/src/plan_since.rs': 'plan --since-run: the failed jobs and tests of one run, and the marker reader',
  'tools/paddock-dev/src/plan_cmd.rs': 'plan: the base of a diff is the last green push run of main (what to test, not whether to release)',
  'scripts/affected.mjs': 'the node fallback of the planner: the same last-green base as plan_cmd.rs, deleted with the script (ADR 0022 R4)',
  'scripts/ci-timings.mjs': 'the script ci timings was ported from',
  'conformance/ci/release-verdict.mjs': 'reads the current run\'s own jobs to say whether installers were built, not a verdict on a commit',
  'scripts/vm-smoke/run.mjs': 'downloads a run\'s artifacts',
  'scripts/vm-smoke/run-linux.mjs': 'downloads a run\'s artifacts',
  'scripts/vm-smoke/run-win.mjs': 'downloads a run\'s artifacts',
  'scripts/vm-smoke/report.mjs': 'reads a run only to name the commit of an installer under test',
};

const SKIP_DIR = new Set(['node_modules', 'target', '.git', 'tests', 'fixtures', 'dist']);
const SOURCE = /\.(?:mjs|js|sh|rs|py|ts|yml|yaml)$/;
const TEST_FILE = /\.test\.(?:mjs|js|ts|sh)$/;

/** The source files under `dirs` of `root` whose text reads the runs API and are not in `allowed`. */
export function secondReaders(root, dirs = ['conformance/ci', 'conformance/src', 'scripts', 'tools', '.gitea'], allowed = ALLOWED) {
  const found = [];
  const walk = (dir) => {
    for (const name of readdirSync(dir)) {
      const p = join(dir, name);
      if (statSync(p).isDirectory()) {
        if (!SKIP_DIR.has(name)) walk(p);
      } else if (SOURCE.test(name) && !TEST_FILE.test(name)) {
        const rel = relative(root, p).split('\\').join('/');
        if (READER.test(readFileSync(p, 'utf8')) && !(rel in allowed)) found.push(rel);
      }
    }
  };
  for (const d of dirs) if (existsSync(join(root, d))) walk(join(root, d));
  return found.sort();
}

test('no script or tool reads the Actions runs API for a pass/fail verdict except ./dev ci verdict (allow-list with reasons)', () => {
  assert.deepEqual(
    secondReaders(PADDOCK),
    [],
    'a second reader of the runs: ask `./dev ci verdict --workflow <file> --sha <sha>` instead (exit 0 full green, 5 red or partial, 6 not measured); if it reads runs for another purpose, add it to ALLOWED in conformance/test/ci-verdict-reader.test.mjs with the reason',
  );
});

test('the allow-list holds no stale entry: every file exists and still reads the runs', () => {
  for (const [rel, why] of Object.entries(ALLOWED)) {
    assert.ok(why.length > 10, `${rel}: say why`);
    const f = join(PADDOCK, rel);
    assert.ok(existsSync(f), `${rel} no longer exists: remove it from ALLOWED`);
    assert.match(readFileSync(f, 'utf8'), READER, `${rel} no longer reads the runs API: remove it from ALLOWED`);
  }
});

test('the scan sees a second reader (positive control)', async () => {
  const { makeApp } = await import('./helpers.mjs');
  const root = makeApp({
    'scripts/bad.mjs': 'const r = await fetch(`${api}/repos/o/r/actions/runs?head_sha=${sha}`);\n',
    'scripts/rust-bad.rs': 'use crate::ci_runs::client::Client;\n',
    'scripts/good.mjs': 'console.log("nothing");\n',
    'scripts/bad.test.mjs': 'actions/runs\n',
    'scripts/allowed.mjs': 'actions/runs\n',
  });
  assert.deepEqual(secondReaders(root, ['scripts'], { 'scripts/allowed.mjs': 'a reason that is long enough' }), ['scripts/bad.mjs', 'scripts/rust-bad.rs']);
});
