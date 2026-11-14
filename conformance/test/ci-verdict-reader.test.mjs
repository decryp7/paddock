// `./dev ci verdict` (tools/paddock-dev/src/ci_runs/verdict.rs) is the one reader of the Actions runs for "is there a full green run
// of workflow W on commit S?". Two checks keep it that way: every workflow the tag gate can ask about prints the full-mode marker
// (in its own plan job, or through the shared plan action, whose callers pass the scope as `full:`), and no other script or tool
// reads the runs API to make a pass/fail verdict.
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
const codeLines = (body) =>
  body
    .split('\n')
    .filter((l) => !l.trim().startsWith('#'))
    .map((l) => l.trim())
    .filter(Boolean);
const planCode = (text) => codeLines(jobText(text, 'plan') ?? '');

/** Why a workflow's plan job does not print the verdict marker after its decision, or null when it does. */
export function markerProblem(text, code = planCode(text), { sinceRun = true } = {}) {
  if (code.length === 0) return 'it has no `plan` job';
  const at = code.indexOf(VERDICT_ECHO);
  const mentions = code.filter((l) => /^(?:echo|printf)\b.*plan verdict:/.test(l));
  if (mentions.length > 1 || (mentions.length === 1 && at < 0)) return `its plan job prints \`plan verdict:\` other than as the single \`${VERDICT_ECHO}\` after its decision: ${mentions.join(' | ')}`;
  if (at < 0) return `its plan job does not \`${VERDICT_ECHO}\` (plan_since::COMPLETE_LINE)`;
  const decided = code.findLastIndex((l) => /^(?:\.\/dev plan\b|run=\$\(|echo "run=|node scripts\/affected\.mjs|grep -q|if \[ "\$EVENT")/.test(l) || /^echo "(?:run|crate_legs|paddock_dev_legs)=/.test(l) || /\|\| echo "(?:crate_legs|paddock_dev_legs)=/.test(l));
  if (decided > at) return 'its plan job prints the verdict line before it has finished deciding';
  const cond = sinceRun ? /^if \[.*-z "\$PLAN_SINCE_RUN".*PLAN_MODE_FULL" = true.*complete=true/ : /^if \[ "\$run" = true \] && \[ "\$PLAN_MODE_FULL" = true \]; then complete=true; fi$/;
  if (!code.some((l) => cond.test(l))) return 'its complete=true condition does not need ' + (sinceRun ? 'an empty PLAN_SINCE_RUN and ' : '') + 'PLAN_MODE_FULL = true';
  return null;
}

/** The three lines that decide `complete` (from `complete=false` to the echo) of a plan job. */
export function verdictSnippet(text, code = planCode(text)) {
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

const ACTION = 'actions/plan/action.yml';
const actionText = () => readFileSync(join(PADDOCK, '.gitea', ACTION), 'utf8');
/** Release workflows whose plan job is the shared plan action (it prints the verdict line; the workflow passes `full`). */
const ACTION_WORKFLOWS = ['dyno-release.yml', 'pitcrew-release.yml', 'pitwall-release.yml', 'spotter-release.yml'];

const text = (w) => readFileSync(join(PADDOCK, '.gitea', 'workflows', w), 'utf8');

test('every gate workflow prints the verdict marker after its plan decided (the tag gate reads it through ./dev ci verdict)', () => {
  const wfs = gateWorkflows(PADDOCK);
  assert.ok(wfs.includes('ci.yml') && wfs.includes('debrief.yml'), wfs.join(', '));
  for (const w of wfs) {
    const p = markerProblem(text(w));
    assert.equal(p, null, `${w}: ${p}. Without the marker a dispatch run is never full, so the tag workflow refuses the app's tag as "not full".`);
  }
});

test('the shared plan action prints the verdict marker after deciding, and its release callers pass the scope', () => {
  const code = codeLines(actionText());
  assert.equal(markerProblem(null, code, { sinceRun: false }), null);
  assert.ok(code.includes('PLAN_MODE_FULL: ${{ inputs.full }}'), 'the action takes PLAN_MODE_FULL from its `full` input');
  assert.ok(!code.some((l) => l.includes('PLAN_SINCE_RUN')), 'the action has no since-run input (the workflows that rerun do not use it)');
  for (const w of ACTION_WORKFLOWS) {
    const plan = jobText(text(w), 'plan') ?? '';
    assert.match(plan, /uses: \.\/\.gitea\/actions\/plan/, `${w}: its plan job uses the plan action`);
    assert.match(plan, /^\s+full: \S/m, `${w}: its plan job passes \`full:\` to the plan action (the scope of the run)`);
    assert.doesNotMatch(plan, /plan verdict/, `${w}: no hand-written copy of the verdict line`);
    // The scope is exactly the condition under which the workflow's gates job runs (its `if:` less the plan's `run` term).
    const full = /^\s+full: \$\{\{ (.+) \}\}\s*$/m.exec(plan)?.[1];
    const gatesIf = /^ {4}if: \$\{\{ needs\.plan\.outputs\.run == 'true' && \((.+)\) \}\}\s*$/m.exec(jobText(text(w), 'gates') ?? '')?.[1];
    assert.ok(full && gatesIf, `${w}: could not read the full: expression or the gates job's if: (${full} / ${gatesIf})`);
    assert.equal(full, gatesIf, `${w}: full: must equal the gates job's condition`);
  }
  const snippet = verdictSnippet(null, code);
  const say = (env) => runSnippet(snippet, { run: 'true', PLAN_MODE_FULL: 'true', ...env });
  assert.equal(say({}), 'plan verdict: complete=true');
  assert.equal(say({ run: 'false' }), 'plan verdict: complete=false');
  assert.equal(say({ PLAN_MODE_FULL: 'false' }), 'plan verdict: complete=false');
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
  assert.match(markerProblem(ok.replace('          complete=false\n', '          echo "plan verdict: complete=true"\n          complete=false\n')), /other than as the single/);
  assert.match(markerProblem('jobs:\n  other:\n    x: 1\n'), /no `plan` job/);
  // The scope line alone is not the marker.
  assert.match(markerProblem(ok.replace('plan verdict: complete=', 'plan mode: full=')), /does not/);
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
