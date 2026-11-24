// A member run's nextest filter (app-gates `filter`) can select no test in one of the two nextest runs (the workspace, the last
// packages). That half is skipped with a notice, never run with --no-tests=pass, which would also pass a filter that matches nothing
// by mistake (ER-33); a `nextest list` that fails is not an empty selection, and the run after it fails visibly.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { loadAction, stepsOf } from './lib/workflow.mjs';

const rustStep = () => stepsOf(loadAction('app-gates')).find((s) => s.run?.includes('cargo nextest run'));

/**
 * Runs the Rust step with a fake `cargo` that logs its calls; `list` is what `cargo nextest list` prints (or "FAIL"). A fake `dev`
 * at the workspace root logs its calls too and exits with `devExit` (the rerun check's verdict).
 */
function run({ filter = '', testsOnly = '', last = '', list = '', lastList = '', graph = '', devExit = 0 }) {
  const dir = mkdtempSync(join(tmpdir(), 'app-gates-'));
  const calls = join(dir, 'calls');
  writeFileSync(join(dir, 'dev'), ['#!/bin/bash', `echo "dev $*" >> "${calls}"`, `exit ${devExit}`, ''].join('\n'));
  chmodSync(join(dir, 'dev'), 0o755);
  writeFileSync(
    join(dir, 'cargo'),
    [
      '#!/bin/bash',
      `echo "cargo $*" >> "${calls}"`,
      'if [ "$1 $2" = "nextest list" ]; then',
      '  if [ "$FAKE_LIST" = FAIL ]; then exit 2; fi',
      '  case " $* " in *" -p "*) printf "%s" "$FAKE_LAST";; *) printf "%s" "$FAKE_LIST";; esac',
      'fi',
      'exit 0',
      '',
    ].join('\n'),
  );
  chmodSync(join(dir, 'cargo'), 0o755);
  const r = spawnSync('bash', ['--noprofile', '--norc', '-e', '-o', 'pipefail', '-c', rustStep().run], {
    encoding: 'utf8',
    cwd: dir,
    env: {
      PATH: `${dir}:${process.env.PATH}`, RUNNER_OS: 'Linux', GITHUB_WORKSPACE: dir, RUNNER_TEMP: dir, FAKE_LIST: list, FAKE_LAST: lastList,
      NEXTEST_CONFIG: '', FILTER: filter, TESTS_ONLY: testsOnly, GRAPH_FILTER: graph, DISPLAY_TESTS: '', LAST_PACKAGES: last, WINDOWS_CLIPPY: '',
    },
  });
  let log = '';
  try { log = readFileSync(calls, 'utf8'); } catch { /* no cargo call */ }
  return { ...r, calls: log.split('\n').filter(Boolean) };
}

const runs = (r) => r.calls.filter((c) => c.startsWith('cargo nextest run'));

test('a member filter that selects tests runs the workspace half with the filter', () => {
  const r = run({ filter: 'package(=x-a)', list: 'x-a tests::one\n' });
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.equal(runs(r).length, 1, r.calls.join('\n'));
  assert.match(runs(r)[0], /-E package\(=x-a\)/);
  assert.doesNotMatch(runs(r)[0], /no-tests/);
});

test('a member filter that selects nothing skips that half with a notice, and is not run as a pass', () => {
  const r = run({ filter: 'package(=x-a)', list: '' });
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.equal(runs(r).length, 0, r.calls.join('\n'));
  assert.match(r.stdout, /::notice::nextest: no test of the workspace matches the member filter \(package\(=x-a\)\); this half is skipped, not passed/);
});

test('the last packages are listed apart: an empty one is skipped with a notice, the workspace half still runs', () => {
  const r = run({ filter: 'package(=x-a)', last: 'x-agent', list: 'x-a tests::one\n', lastList: '' });
  assert.equal(runs(r).length, 1, r.calls.join('\n'));
  assert.doesNotMatch(runs(r)[0], /-p x-agent/);
  assert.match(r.stdout, /no test of the last packages \(x-agent\) matches the member filter/);
  // Positive control: when the last packages select a test too, both halves run.
  const both = run({ filter: 'package(=x-agent)', last: 'x-agent', list: 'x-a tests::one\n', lastList: 'x-agent t\n' });
  assert.equal(runs(both).length, 2, both.calls.join('\n'));
});

test('a nextest list that fails is not an empty selection: the run goes on and fails visibly', () => {
  const r = run({ filter: 'package(=x-a)', list: 'FAIL' });
  assert.equal(runs(r).length, 1, `the run must still be attempted\n${r.calls.join('\n')}`);
});

test('without a member filter nothing is listed and the full run is not narrowed (positive control)', () => {
  const r = run({});
  assert.equal(r.calls.filter((c) => c.startsWith('cargo nextest list')).length, 0, r.calls.join('\n'));
  assert.equal(runs(r).length, 1);
  assert.doesNotMatch(runs(r)[0], /-E /);
});

test('a rerun of failed tests checks each named test before it runs them, and a name that selects nothing fails (run 8868)', () => {
  const filter = '(binary_id(=a) & test(=m::b))';
  const r = run({ filter, testsOnly: 'true', list: '{}' });
  assert.equal(r.status, 0, r.stdout + r.stderr);
  const list = r.calls.filter((c) => c.startsWith('cargo nextest list'));
  assert.equal(list.length, 1, r.calls.join('\n'));
  assert.match(list[0], /--workspace .*-E \(binary_id\(=a\) & test\(=m::b\)\) --message-format json/);
  assert.ok(r.calls.some((c) => c.startsWith(`dev ci rerun-check --filter ${filter} --listed `)), r.calls.join('\n'));
  assert.match(runs(r)[0], /--no-tests=fail/, r.calls.join('\n'));
  // A named test that selects nothing (renamed since the failed run): the step fails before nextest runs, never a warning.
  const gone = run({ filter, testsOnly: 'true', list: '{}', devExit: 5 });
  assert.equal(gone.status, 1, gone.stdout + gone.stderr);
  assert.equal(runs(gone).length, 0, gone.calls.join('\n'));
  // A listing that fails cannot be checked: the step fails with the cause, nothing runs.
  const broken = run({ filter, testsOnly: 'true', list: 'FAIL' });
  assert.equal(broken.status, 1);
  assert.match(broken.stdout, /::error::rerun-check: cargo nextest list failed/);
  assert.equal(runs(broken).length, 0);
  // Positive control: a full run (no rerun) neither lists nor checks.
  assert.ok(!run({}).calls.some((c) => c.startsWith('dev ')));
});

test("the graph's filterset (graph-filter) that selects no test in either half fails, never passes (review round 2 P2-2)", () => {
  const none = run({ filter: 'binary_id(=x-a::it)', graph: 'true', list: '' });
  assert.equal(none.status, 1, none.stdout + none.stderr);
  assert.match(none.stdout, /::error::the graph's selection \(binary_id\(=x-a::it\)\) selects no test of this workspace/);
  // Positive control: a graph filterset that selects a test runs it and passes, with the gates (fmt, clippy) before it.
  const one = run({ filter: 'binary_id(=x-a::it)', graph: 'true', list: 'x-a::it t\n' });
  assert.equal(one.status, 0, one.stdout + one.stderr);
  assert.equal(runs(one).length, 1, one.calls.join('\n'));
  assert.ok(one.calls.some((c) => c.startsWith('cargo clippy')) && one.calls.some((c) => c.startsWith('cargo fmt')), one.calls.join('\n'));
  // The same empty selection as a member filter (not the graph's) is skipped with a notice, as before.
  assert.equal(run({ filter: 'binary_id(=x-a::it)', list: '' }).status, 0);
});
