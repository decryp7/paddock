// `./dev ci verdict` (tools/paddock-dev/src/ci_runs/verdict.rs) is the one reader of the Actions runs for "is there a full green run
// of workflow W on commit S?". Two checks keep it that way: every workflow the tag gate can ask about prints the full-mode marker
// in its plan job, and no other script or tool reads the runs API to make a pass/fail verdict.
import assert from 'node:assert/strict';
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { test } from 'node:test';
import { PADDOCK } from './helpers.mjs';

// ---- the marker ---------------------------------------------------------------------------------------------------------------
/** The text of one top-level job of a workflow, or null. */
export function jobText(text, id) {
  const lines = text.split('\n');
  const start = lines.findIndex((l) => l === `  ${id}:`);
  if (start < 0) return null;
  const end = lines.findIndex((l, i) => i > start && /^ {2}[\w-]+:/.test(l));
  return lines.slice(start, end < 0 ? undefined : end).join('\n');
}

/** Why a workflow's plan job does not print the full-mode marker, or null when it does. */
export function markerProblem(text) {
  const plan = jobText(text, 'plan');
  if (plan == null) return 'it has no `plan` job';
  const code = plan.split('\n').filter((l) => !l.trim().startsWith('#')).join('\n');
  if (!/^\s*PLAN_MODE_FULL:\s*\S/m.test(code)) return 'its plan job sets no PLAN_MODE_FULL';
  if (!/echo "plan mode: full=\$\{?PLAN_MODE_FULL\}?"/.test(code)) return 'its plan job does not `echo "plan mode: full=$PLAN_MODE_FULL"` (plan_since::MODE_LINE)';
  return null;
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

test('every gate workflow prints the full-mode marker in its plan job (the tag gate reads it through ./dev ci verdict)', () => {
  const wfs = gateWorkflows(PADDOCK);
  assert.ok(wfs.includes('ci.yml') && wfs.includes('debrief.yml'), wfs.join(', '));
  for (const w of wfs) {
    const p = markerProblem(readFileSync(join(PADDOCK, '.gitea', 'workflows', w), 'utf8'));
    assert.equal(p, null, `${w}: ${p}. Without the marker a dispatch run is never full, so the tag workflow refuses the app's tag as "not full".`);
  }
});

test('the marker check sees a missing or commented-out marker (positive control)', () => {
  const ok = 'jobs:\n  plan:\n    steps:\n      - env:\n          PLAN_MODE_FULL: ${{ true }}\n        run: |\n          echo "plan mode: full=$PLAN_MODE_FULL"\n  next:\n    x: 1\n';
  assert.equal(markerProblem(ok), null);
  assert.match(markerProblem(ok.replace('echo "plan mode', '# echo "plan mode')), /does not/);
  assert.match(markerProblem(ok.replace('PLAN_MODE_FULL: ${{ true }}', 'X: 1')), /sets no PLAN_MODE_FULL/);
  assert.match(markerProblem('jobs:\n  other:\n    x: 1\n'), /no `plan` job/);
});

// ---- no second reader ---------------------------------------------------------------------------------------------------------
/** What reads the Actions runs: the API paths, the run list's JSON key, and the Rust client types. */
export const READER = /actions\/(?:runs|jobs)\b|workflow_runs|GiteaRuns|ci_runs::client|\.runs_page/;

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
  'conformance/src/checks/C07-release-preflight.mjs': 'the legacy --preflight (no workflow runs it): move it onto ./dev ci verdict or delete it before it is wired',
  'scripts/vm-smoke/run.mjs': 'downloads a run\'s artifacts',
  'scripts/vm-smoke/run-linux.mjs': 'downloads a run\'s artifacts',
  'scripts/vm-smoke/run-win.mjs': 'downloads a run\'s artifacts',
  'scripts/vm-smoke/report.mjs': 'reads a run only to name the commit of an installer under test',
};

const SKIP_DIR = new Set(['node_modules', 'target', '.git', 'tests', 'fixtures', 'dist']);
const SOURCE = /\.(?:mjs|js|sh|rs|py|ts)$/;
const TEST_FILE = /\.test\.(?:mjs|js|ts|sh)$/;

/** The source files under `dirs` of `root` whose text reads the runs API and are not in `allowed`. */
export function secondReaders(root, dirs = ['conformance/ci', 'conformance/src', 'scripts', 'tools'], allowed = ALLOWED) {
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
