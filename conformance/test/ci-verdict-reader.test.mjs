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
import { baselineGrowth, isCi } from '../src/util/baseline.mjs';
import { isReportOnly, reportOnlyRows } from '../src/util/gates-register.mjs';
import { conjuncts, expr, job as workflowJob, loadWorkflow, stepsUsing } from './lib/workflow.mjs';

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
    const wf = loadWorkflow(w);
    const full = expr(stepsUsing(workflowJob(wf, 'plan'), './.gitea/actions/plan')[0]?.with?.full);
    const [runTerm, scope, ...rest] = conjuncts(workflowJob(wf, 'gates').if);
    assert.equal(runTerm, "needs.plan.outputs.run == 'true'", `${w}: the gates job runs only when the plan says so`);
    assert.ok(full && scope && rest.length === 0, `${w}: could not read the full: expression or the gates job's if: (${full} / ${scope})`);
    assert.equal(full, scope.replace(/^\((.*)\)$/, '$1'), `${w}: full: must equal the gates job's condition`);
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

// ---- no second pass/fail decision from a run's conclusion -----------------------------------------------------------------------
// A run that ended `success` is not "green" (a rerun of failures, a schedule-skipped run and a partial run all end `success`), and a
// run that ended `cancelled` is neither green nor red. The pass decision of one run lives in tools/paddock-dev/src/ci_runs/verdict.rs
// (`run_result`, `decide`) and nowhere else; the train commands ask it. Files that still compare a conclusion with "success" are
// listed in conformance/ci/run-conclusion-readers.json: the list only shrinks (ER-46), a listed file that no longer matches is
// stale, and a new file that matches fails here.

/** A comparison of a conclusion with "success" that decides something (`== "success"`, `matches!(... "success" ...)`, `=== 'success'`). */
export const PASS_DECISION =
  /conclusion[^\n]{0,60}?(?:===?|!==?)\s*['"]success['"]|['"]success['"]\s*(?:===?|!==?)[^\n]{0,60}?conclusion|matches!\([^\n]*conclusion[^\n]*['"]success['"]|conclusion[^\n]{0,40}\bSome\(\s*['"]success['"]|status\s*==\s*['"]success['"]/;
const DECISION_HOME = 'tools/paddock-dev/src/ci_runs/verdict.rs';
const DECISION_DIRS = ['tools/paddock-dev/src', 'scripts', 'conformance/src', 'conformance/ci', '.gitea'];

/** The files under `dirs` of `root` that compare a run's conclusion with "success", outside the decision's home. */
export function conclusionReaders(root, dirs = DECISION_DIRS, home = DECISION_HOME) {
  return sources(root, dirs)
    .filter(([rel, text]) => rel !== home && PASS_DECISION.test(text))
    .map(([rel]) => rel)
    .sort();
}

const READERS_LIST = 'conformance/ci/run-conclusion-readers.json';

/**
 * ER-41 (amended 2026-11-20): a new check starts report-only and blocks only in the step that records its first green run on the real
 * tree. Its status is the row `[[report_only]] id = "test:conformance/test/ci-verdict-reader.test.mjs"` of docs/gates.toml, read through the one accessor
 * (`gates-register.mjs`): while the row is there every finding of this check (a new reader, a stale entry, growth of the list, its
 * creation) is a warning; the step that records the first green run removes the row, and the findings fail.
 */
export const REGISTER_ID = 'test:conformance/test/ci-verdict-reader.test.mjs';
const REGISTER = 'docs/gates.toml';
const blockingNow = (extra = {}) => extra.blocking ?? !isReportOnly(readFileSync(join(PADDOCK, REGISTER), 'utf8'), REGISTER_ID);

/**
 * The shrink-only rule of the list (ER-46) against the merge base with the target ref (`targetRef`: PADDOCK_BASE_REF, else
 * origin/main), through the shared baseline reader. An entry not in the base list throws. A list that does not exist at the base is
 * its own creation, which is growth that needs the user's yes: it throws unless the list records that approval (`approved`: who,
 * the date and the exact entries approved) and every entry listed is one of them. Once the list is on the target branch this path
 * is never taken again. Where the base cannot be read it warns, and in CI it throws (not measured is never a pass).
 */
export function listGrowth(listed, extra = {}) {
  const warn = extra.warn ?? console.warn;
  const approved = extra.approved;
  // Report-only until the first green run is recorded (ER-41): a finding is said, not thrown.
  const fail = (message) => {
    if (blockingNow(extra)) throw new Error(message);
    warn(`report-only (ER-41, docs/gates.toml lists this test): ${message}`);
  };
  const g = baselineGrowth({ root: PADDOCK, path: READERS_LIST, current: listed, keyOf: (f) => f, entriesOf: (j) => j.files, ...extra });
  if (g.reason) {
    if (extra.ci ?? isCi()) throw new Error(`${READERS_LIST}: not measured, the target ref is not usable: ${g.reason}`);
    warn(`${READERS_LIST}: the shrink-only rule is not enforced here: ${g.reason}`);
  } else if (g.absent) {
    const ok = approved?.by && /^\d{4}-\d{2}-\d{2}$/.test(approved.date ?? '') && Array.isArray(approved.files);
    const extraEntries = ok ? listed.filter((f) => !approved.files.includes(f)) : listed;
    if (extraEntries.length) {
      fail(`${READERS_LIST} is a new list and its creation is growth that needs the user's approval (ER-46): ${ok ? 'not approved' : 'no approval is recorded'}: ${extraEntries.slice(0, 5).join('; ')}. Record it as "approved": {by, date, files} only after the user said yes.`);
      return g;
    }
    warn(`${READERS_LIST}: the creation of this list was approved by ${approved.by} on ${approved.date} (${listed.length} entries); it is not asked again once it is on ${'the target branch'}`);
  } else if (g.added.length) {
    fail(`${READERS_LIST} grew: ${g.added.length} entr${g.added.length === 1 ? 'y is' : 'ies are'} not in the baseline at the merge base (${g.added.slice(0, 5).join('; ')}). It may only shrink.`);
  }
  return g;
}

test('a pass decision from a run conclusion lives only in ci_runs/verdict.rs, besides the listed old readers, which only shrink', () => {
  const listed = JSON.parse(readFileSync(join(PADDOCK, READERS_LIST), 'utf8')).files;
  const found = conclusionReaders(PADDOCK);
  const check = (actual, message) => {
    if (blockingNow()) assert.deepEqual(actual, [], message);
    else if (actual.length) console.warn(`report-only (ER-41, docs/gates.toml lists this test): ${message}: ${actual.join('; ')}`);
  };
  check(
    found.filter((f) => !listed.includes(f)),
    'a new reader of a run conclusion: ask `ci_runs::verdict::run_result` (one run) or `ci_runs::verdict::verdict` (is there a full green run); the list in conformance/ci/run-conclusion-readers.json never grows',
  );
  assert.deepEqual(
    listed.filter((f) => !found.includes(f)),
    [],
    'a listed file no longer reads a conclusion: remove it from conformance/ci/run-conclusion-readers.json (the list only shrinks)',
  );
  // ER-46: against the merge base, never by this test's own memory. The list's first creation is growth until the user approves it.
  listGrowth(listed, { approved: JSON.parse(readFileSync(join(PADDOCK, READERS_LIST), 'utf8')).approved });
});

const gitBase = (base) => (args) => {
  if (args[0] === 'rev-parse') return args[1] === 'HEAD' ? 'def\n' : 'abc\n';
  if (args[0] === 'merge-base') return 'abc\n';
  if (args[0] === 'show') {
    if (base === null) throw Object.assign(new Error('fatal: path does not exist'), { stderr: 'fatal: path does not exist' });
    return base;
  }
  throw new Error(`unexpected git ${args.join(' ')}`);
};

test('the shrink-only check sees an added entry, a new list and a target it cannot read (positive control)', () => {
  const base = JSON.stringify({ files: ['a.rs', 'b.rs'] });
  assert.doesNotThrow(() => listGrowth(['a.rs'], { git: gitBase(base), blocking: true }), 'shrinking is fine');
  assert.throws(() => listGrowth(['a.rs', 'c.rs'], { git: gitBase(base), blocking: true }), /grew.*c\.rs/, 'an added entry is growth');
  // A creation: refused without a recorded approval, refused for an entry the approval does not name, allowed when it covers them.
  const approval = { by: 'the user', date: '2026-11-20', files: ['a.rs', 'b.rs'] };
  assert.throws(() => listGrowth(['a.rs', 'b.rs'], { git: gitBase(null), blocking: true }), /no approval is recorded/, 'a new list with no approval fails');
  assert.throws(() => listGrowth(['a.rs', 'c.rs'], { git: gitBase(null), approved: approval, blocking: true }), /not approved: c\.rs/, 'an entry the approval does not name fails');
  assert.throws(() => listGrowth(['a.rs'], { git: gitBase(null), approved: { ...approval, date: 'soon' }, blocking: true }), /no approval is recorded/, 'an approval needs its date');
  const said = [];
  const created = listGrowth(['a.rs', 'b.rs'], { git: gitBase(null), approved: approval, warn: (m) => said.push(m) });
  assert.equal(created.absent, true);
  assert.match(said[0], /approved by the user on 2026-11-20/);
  // Report-only (the default until the first green run is recorded): the same findings are said, not thrown.
  const spoken = [];
  assert.doesNotThrow(() => listGrowth(['a.rs', 'c.rs'], { git: gitBase(base), warn: (m) => spoken.push(m) }));
  assert.doesNotThrow(() => listGrowth(['a.rs'], { git: gitBase(null), warn: (m) => spoken.push(m) }));
  assert.equal(spoken.filter((m) => m.includes('report-only')).length, 2, 'both findings were reported');
  // The status comes from the register's row, not from a constant: with the row the check reports, without it the check blocks.
  const row = '[[report_only]]\nid = "test:conformance/test/ci-verdict-reader.test.mjs"\nsince = 2026-11-20\n';
  assert.equal(isReportOnly(row, REGISTER_ID), true);
  assert.equal(isReportOnly('[[report_only]]\nid = "C122"\n', REGISTER_ID), false);
  assert.equal(isReportOnly(readFileSync(join(PADDOCK, REGISTER), 'utf8'), REGISTER_ID), true, 'the register records the report-only start');
  const noGit = () => {
    throw new Error('not a git repository');
  };
  listGrowth(['a.rs'], { git: noGit, ci: false, warn: () => {} });
  assert.throws(() => listGrowth(['a.rs'], { git: noGit, ci: true }), /not measured/, 'in CI an unreadable target is not a pass');
});

test('the conclusion scan sees a pass decision in Rust and in JavaScript, and not the home or a mere mention (positive control)', async () => {
  const { makeApp } = await import('./helpers.mjs');
  const root = makeApp({
    'tools/paddock-dev/src/train/bad.rs': 'if run.conclusion == "success" { go(); }\n',
    'tools/paddock-dev/src/train/bad2.rs': 'if matches!(r.conclusion.as_str(), "success") { go(); }\n',
    'scripts/bad.mjs': "if (run.conclusion === 'success') go();\n",
    'tools/paddock-dev/src/ci_runs/verdict.rs': 'if run.conclusion == "success" {}\n',
    'tools/paddock-dev/src/train/good.rs': '// a run ends success or failure\nlet x = run_result(&r);\n',
  });
  assert.deepEqual(conclusionReaders(root), [
    'scripts/bad.mjs',
    'tools/paddock-dev/src/train/bad.rs',
    'tools/paddock-dev/src/train/bad2.rs',
  ]);
});

test('the register reader sees report_only rows, skips other tables and reads dates and quoted text (positive control)', () => {
  const text = '[[not_gated]]\nid = "C03"\n\n[[report_only]]\nid = "x"\nreason = "a \\"quoted\\" word" # why\nsince = 2026-11-20\n\n[[report_only]]\nid = "y"\n';
  const rows = reportOnlyRows(text);
  assert.deepEqual(rows.map((r) => r.id), ['x', 'y']);
  assert.equal(rows[0].reason, 'a "quoted" word');
  assert.equal(rows[0].since, '2026-11-20');
  assert.equal(isReportOnly(text, 'C03'), false, 'a not_gated row is not report-only');
  assert.equal(isReportOnly('', 'x'), false);
});
