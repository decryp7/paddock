// The CI diagnostics helpers (rule 56): conformance/ci/ci-log.sh for bash steps, conformance/ci/ci-log.mjs for Node scripts.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { ciError, formatDuration, formatError, phase, writeSummary } from '../ci/ci-log.mjs';

const LIB = join(dirname(fileURLToPath(import.meta.url)), '..', 'ci', 'ci-log.sh');

/** Runs `script` in bash with ci-log.sh sourced and `set -uo pipefail`, as the CI steps do. */
function bash(script, env = {}) {
  return spawnSync('bash', ['-c', `set -uo pipefail; . "${LIB}"; ${script}`], {
    env: { PATH: '/usr/bin:/bin', ...env },
    encoding: 'utf8',
  });
}

function scratch() {
  const dir = realpathSync(mkdtempSync(join(tmpdir(), 'paddock-ci-log-')));
  return { dir, done: () => rmSync(dir, { recursive: true, force: true }) };
}

test('ci_error prints one ::error:: line with the cause and the next step, newlines folded', () => {
  const r = bash(`ci_error $'checksum of node.tar.gz differs\\nwant abc' 're-run once'`);
  assert.equal(r.status, 0);
  assert.equal(r.stdout, '::error::checksum of node.tar.gz differs / want abc. Next step: re-run once\n');
  assert.equal(bash(`ci_error 'only a cause'`).stdout, '::error::only a cause\n');
});

test('the ::error:: prefix is assembled at run time, so the helper\'s source never matches a grep of the annotation', () => {
  const src = readFileSync(LIB, 'utf8');
  const code = src.split('\n').filter((l) => !l.trimStart().startsWith('#')).join('\n');
  assert.doesNotMatch(code, /::error::/, 'no literal annotation in the code a step log could echo');
});

test('ci_describe_exit says what a status means: not found or a hidden crash, not executable, a signal', () => {
  const d = (c) => bash(`ci_describe_exit ${c}`).stdout.trim();
  assert.equal(d(0), 'exit 0');
  assert.equal(d(2), 'exit 2');
  assert.match(d(127), /command not found.*run-native\.mjs/);
  assert.match(d(126), /not executable/);
  assert.match(d(143), /^exit 143 \(killed by signal 15 SIGTERM\)$/);
  assert.match(d(137), /signal 9 SIGKILL/);
});

test('ci_step groups and times a command, keeps its status and names the command, folder and a reproduction on failure', () => {
  const s = scratch();
  try {
    const ok = bash(`cd "${s.dir}"; ci_step "say hi" echo hi; echo "rc=$?"`);
    assert.match(ok.stdout, /^::group::say hi\n\+ echo hi\nhi\n::endgroup::\nci: say hi: ok in \d+\.\d s\nrc=0\n$/);
    const bad = bash(`cd "${s.dir}"; ci_step "fail" sh -c 'echo boom >&2; exit 3'; echo "rc=$?"`);
    assert.match(bad.stdout, /rc=3\n$/, 'the command\'s own status comes back');
    const errors = bad.stdout.split('\n').filter((l) => l.startsWith('::error::'));
    assert.equal(errors.length, 1, 'exactly one annotation per failure');
    assert.match(errors[0], /fail failed \(exit 3, after \d+\.\d s\): `sh -c echo boom >&2; exit 3` in .*paddock-ci-log-/);
    assert.match(errors[0], /Next step: .*reproduce with: \(cd .*paddock-ci-log-\S* && sh -c/);
  } finally {
    s.done();
  }
});

test('ci_summary_flush writes what ran, failed and was skipped to the log and to $GITHUB_STEP_SUMMARY', () => {
  const s = scratch();
  try {
    const file = join(s.dir, 'summary.md');
    writeFileSync(file, '');
    const r = bash(`ci_step a true; ci_step b false; ci_skip "c" "Linux leg only"; ci_summary_flush "Rust (x, Linux)"`, { GITHUB_STEP_SUMMARY: file });
    const md = readFileSync(file, 'utf8');
    assert.match(md, /^### Rust \(x, Linux\)\n\n\| Part \| Result \| Time \|\n\|---\|---\|---\|\n/);
    assert.match(md, /\| a \| ok \| \d+\.\d s \|/);
    assert.match(md, /\| b \| \*\*failed\*\*, exit 1 \| \d+\.\d s \|/);
    assert.match(md, /\| c \| skipped: Linux leg only \| \|/);
    assert.match(r.stdout, /ci: summary of Rust \(x, Linux\):\nci: {3}\| a \| ok/, 'the log has it too (Gitea may not render summaries)');
  } finally {
    s.done();
  }
});

test('ci_summary_flush never fails a step: no summary file, an unwritable one, or nothing recorded (bash 3.2 empty array)', () => {
  assert.equal(bash('ci_summary_flush "nothing"; echo done').stdout, 'done\n');
  const none = bash('ci_step a true >/dev/null; ci_summary_flush t');
  assert.equal(none.status, 0);
  const bad = bash('ci_step a true >/dev/null; ci_summary_flush t; echo "rc=$?"', { GITHUB_STEP_SUMMARY: '/nonexistent-dir/summary.md' });
  assert.match(bad.stdout, /could not write the step summary.*\nrc=0\n$/s);
});

test('_ci_now_ms reads bash 5\'s EPOCHREALTIME to the millisecond (either decimal separator), else whole seconds', () => {
  // In bash 3.2 EPOCHREALTIME is an ordinary variable, so the bash 5 path can be fed a value; in bash 5 it is the live clock.
  const fed = Number(bash('EPOCHREALTIME=1700000000.098765; _ci_now_ms').stdout);
  const comma = Number(bash('EPOCHREALTIME=1700000000,5; _ci_now_ms').stdout);
  const live = Date.now();
  for (const v of [fed, comma]) assert.ok(v === 1700000000098 || v === 1700000000500 || Math.abs(v - live) < 60_000, `${v}`);
  if (fed < live - 60_000) assert.equal(fed, 1700000000098);
  const plain = Number(bash('unset EPOCHREALTIME; _ci_now_ms').stdout);
  assert.ok(Math.abs(plain - live) < 60_000, `${plain}`);
  assert.equal(bash('ci_duration 12345').stdout, '12.3 s');
});

test('run-commands.sh lists the commands skipped after a failure in its summary, with the reason', () => {
  const s = scratch();
  try {
    mkdirSync(join(s.dir, 'sub'));
    const file = join(s.dir, 'summary.md');
    const r = spawnSync('bash', [join(dirname(LIB), 'run-commands.sh')], {
      cwd: s.dir,
      env: { PATH: '/usr/bin:/bin', COMMANDS: 'cd sub\ntrue\nexit 4\ntouch never', GITHUB_STEP_SUMMARY: file, CI_SUMMARY_TITLE: 'checks (x)' },
      encoding: 'utf8',
    });
    assert.equal(r.status, 4);
    const md = readFileSync(file, 'utf8');
    assert.match(md, /### checks \(x\)/);
    assert.match(md, /\| `true` \| ok \|/);
    assert.match(md, /\| `exit 4` \| \*\*failed\*\*, exit 4 \|/);
    assert.match(md, /\| `touch never` \| skipped: an earlier command failed \|/);
  } finally {
    s.done();
  }
});

test('formatError and ciError: one line, cause then next step', () => {
  assert.equal(formatError('a\nb', 'do x'), '::error::a / b. Next step: do x');
  assert.equal(formatError('only'), '::error::only');
  const out = [];
  ciError('c', 'n', (l) => out.push(l));
  assert.deepEqual(out, ['::error::c. Next step: n']);
  assert.equal(formatDuration(12345), '12.3 s');
  assert.equal(formatDuration(-5), '0.0 s');
});

test('phase logs start, outcome and duration, returns the value and rethrows a failure', async () => {
  let t = 1000;
  const now = () => t;
  const lines = [];
  const log = (l) => lines.push(l);
  const v = await phase('plan', () => {
    t += 1500;
    return 42;
  }, { log, now });
  assert.equal(v, 42);
  await assert.rejects(phase('fetch', async () => {
    t += 200;
    throw new Error('HTTP 502');
  }, { log, now }), /HTTP 502/);
  assert.deepEqual(lines, ['ci: plan: started', 'ci: plan: ok in 1.5 s', 'ci: fetch: started', 'ci: fetch: FAILED in 0.2 s: HTTP 502']);
});

test('writeSummary appends to $GITHUB_STEP_SUMMARY when set, always logs, and never throws', () => {
  const s = scratch();
  try {
    const file = join(s.dir, 's.md');
    const lines = [];
    assert.equal(writeSummary('### T\n\nbody', { env: { GITHUB_STEP_SUMMARY: file }, log: (l) => lines.push(l) }), true);
    assert.equal(readFileSync(file, 'utf8'), '### T\n\nbody\n\n');
    assert.deepEqual(lines, ['ci: ### T', 'ci: ', 'ci: body']);
    assert.equal(writeSummary('x', { env: {}, log: () => {} }), false);
    const bad = [];
    assert.equal(writeSummary('x', { env: { GITHUB_STEP_SUMMARY: join(s.dir, 'no', 'such', 'f.md') }, log: (l) => bad.push(l) }), false);
    assert.match(bad.at(-1), /could not write the step summary/);
  } finally {
    s.done();
  }
});
