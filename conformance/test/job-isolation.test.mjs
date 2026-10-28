// Job isolation (.gitea/actions/README.md, "Job isolation"; ADR 0025): conformance/ci/job-isolation.sh with a fake HOME, fake
// runner folders and a fake clock (never the real CI home or runner), and the contract that every host job takes a slot and
// that no CI file reaches a CI-home folder except through the script.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, utimesSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { parseJobs, parseSteps } from '../src/util/workflows.mjs';
import { PADDOCK } from './helpers.mjs';

const SCRIPT = join(PADDOCK, 'conformance', 'ci', 'job-isolation.sh');
const TABLE = join(PADDOCK, 'conformance', 'ci', 'kept-folders.txt');
const read = (p) => readFileSync(p, 'utf8');

/** A fake host: a HOME (the CI home is its .paddock-ci) and a runner work folder with per-job folders. */
function host() {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'paddock-isolation-')));
  mkdirSync(join(root, 'home'));
  /** A job as the runner starts it: <work>/<16 hex>/tmp is its RUNNER_TEMP, and it has its own GITHUB_ENV. */
  const job = (id, extra = {}) => {
    const temp = join(root, 'work', id, 'tmp');
    mkdirSync(temp, { recursive: true });
    const envFile = join(root, 'work', id, 'github-env');
    writeFileSync(envFile, '');
    const base = { PATH: '/usr/bin:/bin', HOME: join(root, 'home'), RUNNER_OS: 'macOS', RUNNER_TEMP: temp, GITHUB_ENV: envFile, GITHUB_RUN_ID: '7', GITHUB_JOB: id, ...extra };
    const exec = (args, more = {}) => spawnSync('bash', [SCRIPT, ...args], { env: { ...base, ...more }, encoding: 'utf8' });
    const exported = () => Object.fromEntries(read(envFile).split('\n').filter(Boolean).map((l) => [l.slice(0, l.indexOf('=')), l.slice(l.indexOf('=') + 1)]));
    /** Runs a command with what acquire exported, as the job's later steps see it. */
    const later = (args, more = {}) => exec(args, { ...exported(), ...more });
    return { id, temp, exec, exported, later, gone: () => rmSync(join(root, 'work', id), { recursive: true, force: true }) };
  };
  return { root, ci: join(root, 'home', '.paddock-ci'), job };
}

test('two jobs on one host get different slots, and each points its temp folders at the runner\'s own folder for it', () => {
  const h = host();
  const a = h.job('aaaaaaaaaaaaaaaa');
  const b = h.job('bbbbbbbbbbbbbbbb');
  assert.equal(a.exec(['acquire']).status, 0);
  const rb = b.exec(['acquire']);
  assert.equal(rb.status, 0, rb.stderr + rb.stdout);
  assert.equal(a.exported().PADDOCK_CI_SLOT, 's1');
  assert.equal(b.exported().PADDOCK_CI_SLOT, 's2');
  assert.equal(a.exported().TMPDIR, a.temp, 'TMPDIR is the runner\'s per-job folder');
  assert.equal(b.exported().PADDOCK_JOB_TEMP, b.temp);
  assert.notEqual(a.later(['path', 'target', 'x-app']).stdout, b.later(['path', 'target', 'x-app']).stdout, 'the same name, two folders');
  assert.equal(a.later(['path', 'target', 'x-app']).stdout.trim(), join(h.ci, 'target-x-app-s1'));
  assert.equal(b.later(['path', 'src', 'x-app-macOS']).stdout.trim(), join(h.ci, 'src', 'x-app-macOS-s2'));
  assert.equal(a.later(['path', 'npm']).stdout.trim(), b.later(['path', 'npm']).stdout.trim(), 'a shared store is one folder for every slot');
  assert.equal(a.later(['acquire']).status, 0, 'a second acquire in the same job keeps its slot');
  assert.equal(a.exported().PADDOCK_CI_SLOT, 's1');
});

test('the slot count follows PADDOCK_CI_SLOTS (capacity 3 or 4 needs no change); when every slot is held by a live job, acquire fails and names them', () => {
  const h = host();
  const jobs = ['1', '2', '3', '4'].map((n) => h.job(n.repeat(16), { PADDOCK_CI_SLOTS: '3' }));
  for (const j of jobs.slice(0, 3)) assert.equal(j.exec(['acquire']).status, 0);
  assert.deepEqual(jobs.slice(0, 3).map((j) => j.exported().PADDOCK_CI_SLOT), ['s1', 's2', 's3']);
  const full = jobs[3].exec(['acquire']);
  assert.notEqual(full.status, 0);
  assert.match(full.stdout, /all 3 slots of this host are held by running jobs/);
  assert.match(full.stdout, /s2: 7\/2222222222222222/);
  assert.equal(jobs[1].later(['release']).status, 0);
  assert.equal(jobs[3].exec(['acquire']).status, 0, 'a released slot is free again');
  assert.equal(jobs[3].exported().PADDOCK_CI_SLOT, 's2', 'the lowest free slot: its kept folders are the warmest');
  const bad = h.job('5'.repeat(16), { PADDOCK_CI_SLOTS: 'many' }).exec(['acquire']);
  assert.notEqual(bad.status, 0);
});

test('a slot is reclaimed only when its owner is gone: its runner folder deleted, too old, or never written; a live job keeps it', () => {
  const h = host();
  const now = 2_000_000_000;
  const a = h.job('a'.repeat(16), { PADDOCK_SLOT_NOW: String(now), PADDOCK_CI_SLOTS: '1' });
  assert.equal(a.exec(['acquire']).status, 0);
  const b = h.job('b'.repeat(16), { PADDOCK_SLOT_NOW: String(now + 60), PADDOCK_CI_SLOTS: '1' });
  assert.notEqual(b.exec(['acquire']).status, 0, 'a live job\'s slot is never taken');
  a.gone(); // the runner deleted the job's folder: the job ended without its release step (cancelled, runner restarted)
  const rb = b.exec(['acquire']);
  assert.equal(rb.status, 0, rb.stdout + rb.stderr);
  assert.match(rb.stdout, /s1 was held by a job that is gone \(7\/a{16}\); reclaimed/);
  assert.equal(b.exported().PADDOCK_CI_SLOT, 's1');
  // Older than the limit (the runner kept its folder): reclaimed.
  const c = h.job('c'.repeat(16), { PADDOCK_SLOT_NOW: String(now + 7 * 3600), PADDOCK_CI_SLOTS: '1' });
  assert.equal(c.exec(['acquire']).status, 0, 'older than PADDOCK_SLOT_MAX_AGE_MIN (6 h)');
  // A lock without an owner file: being written now (kept), or left by a job killed in between (reclaimed after 5 minutes).
  const lock = join(h.ci, 'slots', 's2');
  mkdirSync(lock);
  const d = h.job('d'.repeat(16), { PADDOCK_CI_SLOTS: '2' });
  assert.notEqual(d.exec(['acquire']).status, 0, 'a fresh lock without an owner is being written');
  const old = Date.now() / 1000 - 600;
  utimesSync(lock, old, old);
  assert.equal(d.exec(['acquire']).status, 0);
  assert.equal(d.exported().PADDOCK_CI_SLOT, 's2');
});

test('release frees only the job\'s own slot; a job without one, or in a Linux container, does nothing', () => {
  const h = host();
  const a = h.job('a'.repeat(16));
  a.exec(['acquire']);
  const b = h.job('b'.repeat(16));
  const stolen = b.exec(['release'], { PADDOCK_CI_SLOT: 's1' });
  assert.equal(stolen.status, 0);
  assert.match(stolen.stdout, /s1 is not held by this job; leaving it/);
  assert.ok(existsSync(join(h.ci, 'slots', 's1', 'owner')));
  assert.equal(b.exec(['release']).status, 0, 'no slot: nothing to release');
  assert.equal(a.later(['release']).status, 0);
  assert.ok(!existsSync(join(h.ci, 'slots', 's1')));
  const linux = h.job('l'.repeat(16), { RUNNER_OS: 'Linux' });
  assert.equal(linux.exec(['acquire']).status, 0);
  assert.equal(read(join(h.root, 'work', 'l'.repeat(16), 'github-env')), '', 'a container exports nothing');
  assert.ok(!existsSync(join(h.ci, 'slots', 's2')));
  assert.equal(linux.exec(['path', 'target', 'x']).stdout.trim(), join(h.ci, 'target-x-s1'), 'a container is the single slot s1');
});

test('path: every kind of kept-folders.txt resolves; an unknown kind, a missing or bad name and a host job without a slot fail', () => {
  const h = host();
  const a = h.job('a'.repeat(16));
  a.exec(['acquire']);
  for (const line of read(TABLE).split('\n').filter((l) => l.trim() && !l.startsWith('#'))) {
    const [kind, layout, scope] = line.trim().split(/\s+/);
    assert.match(scope, /^(slot|shared)$/, line);
    if (scope === 'slot') assert.match(layout, /\{slot\}/, `${kind}: a slot folder names its slot`);
    else assert.doesNotMatch(layout, /\{slot\}|\{name\}/, `${kind}: a shared folder is one folder`);
    const r = a.later(['path', kind, 'n']);
    assert.equal(r.status, 0, r.stderr);
    assert.equal(r.stdout.trim(), join(h.ci, layout.replace('{name}', 'n').replace('{slot}', 's1')));
    assert.ok(existsSync(r.stdout.trim()), `${kind} is created`);
  }
  assert.notEqual(a.later(['path', 'nope']).status, 0);
  assert.notEqual(a.later(['path', 'target']).status, 0, 'a name is needed');
  assert.notEqual(a.later(['path', 'target', '../x']).status, 0);
  const none = h.job('b'.repeat(16)).exec(['path', 'target', 'x']);
  assert.notEqual(none.status, 0);
  assert.match(none.stderr, /holds no slot/);
});

test('the CI home follows PADDOCK_CI_HOME (ci-home.sh), and a missing relocated home stops the job instead of filling the internal disk', () => {
  const h = host();
  const disk = join(h.root, 'disk');
  mkdirSync(disk);
  const a = h.job('a'.repeat(16), { PADDOCK_CI_HOME: disk });
  assert.equal(a.exec(['acquire']).status, 0);
  assert.ok(existsSync(join(disk, 'slots', 's1', 'owner')));
  assert.equal(a.later(['path', 'browser', 'spotter']).stdout.trim(), join(disk, 'test-browser-spotter-s1'));
  const gone = h.job('b'.repeat(16), { PADDOCK_CI_HOME: join(h.root, 'unmounted') }).exec(['acquire']);
  assert.notEqual(gone.status, 0);
  assert.ok(!existsSync(join(h.root, 'unmounted')));
});

test('owns, port and pattern scope to the job\'s own slot and folders', () => {
  const h = host();
  const a = h.job('a'.repeat(16));
  a.exec(['acquire']);
  const mine = a.later(['path', 'target', 'x-app']).stdout.trim();
  mkdirSync(join(h.ci, 'target-x-app-s2'));
  assert.equal(a.later(['owns', mine, 'target']).status, 0);
  assert.notEqual(a.later(['owns', join(h.ci, 'target-x-app-s2'), 'target']).status, 0, 'another slot\'s folder');
  assert.notEqual(a.later(['owns', mine, 'src']).status, 0, 'not a folder of that kind');
  assert.notEqual(a.later(['owns', h.ci]).status, 0);
  assert.notEqual(a.later(['owns', join(h.root, 'home')]).status, 0);
  assert.equal(a.later(['port', '4227']).stdout.trim(), '4227');
  assert.equal(a.later(['port', '4320', '10'], { PADDOCK_CI_SLOT: 's3' }).stdout.trim(), '4340');
  const p = a.later(['pattern', '{tmp}/x-{slot}']);
  assert.equal(p.status, 0, p.stderr);
  assert.ok(new RegExp(p.stdout.trim()).test(`${a.temp}/x-s1`));
  assert.ok(!new RegExp(p.stdout.trim()).test(`${a.temp}/x-s2`));
  assert.notEqual(a.later(['pattern', 'gitea-runner']).status, 0, 'a pattern that names no folder of the job is refused');
});

// ---- The contract over every workflow: a host job takes a slot before any kept folder and gives it back last. ----

const WORKFLOWS = join(PADDOCK, '.gitea', 'workflows');
const KEPT = /actions\/(stable-workspace|build-folder|node-caches|setup-rust|host-tools|host-cleanup|prune-build-folder)\b|job-isolation\.sh path/;
const files = spawnSync('git', ['ls-files', '.gitea/workflows/*.yml'], { cwd: PADDOCK, encoding: 'utf8' }).stdout.split('\n').filter(Boolean);

test('every job that may run on a host and uses a kept folder takes a slot first (after Git bash on Windows) and releases it as its last step, always', () => {
  let checked = 0;
  for (const f of files) {
    for (const job of parseJobs(read(join(PADDOCK, f)))) {
      const runsOn = /^\s+runs-on: (.+)$/m.exec(job.text)?.[1] ?? '';
      if (/^ubuntu-latest$/.test(runsOn.trim()) || !KEPT.test(job.text)) continue;
      checked += 1;
      const steps = parseSteps(job.text);
      const acquire = steps.findIndex((s) => /uses: \.\/(\.paddock\/)?\.gitea\/actions\/job-isolation\s*$/m.test(s.text) && !/mode: release/.test(s.text));
      const first = steps.findIndex((s) => KEPT.test(s.text));
      assert.ok(acquire >= 0, `${f} ${job.id}: no job-isolation step (it runs on ${runsOn})`);
      assert.ok(acquire < first, `${f} ${job.id}: the slot is taken before the first kept folder`);
      const gitBash = steps.findIndex((s) => /Git bash on PATH/.test(s.text));
      if (gitBash >= 0) assert.ok(gitBash < acquire, `${f} ${job.id}: Git bash is on PATH before the slot's bash step`);
      const last = steps[steps.length - 1].text;
      assert.match(last, /uses: \.\/(\.paddock\/)?\.gitea\/actions\/job-isolation\n\s+if: always\(\)\n\s+with:\n\s+mode: release/, `${f} ${job.id}: the last step releases the slot, always`);
    }
  }
  assert.ok(checked >= 9, `found the host jobs (${checked})`);
});

/** Every file that runs on a host runner: the workflows, the shared actions, the CI scripts of the kit, tools and apps. */
function ciFiles() {
  const listed = spawnSync('git', ['ls-files', '.gitea/workflows/*.yml', '.gitea/actions/*/action.yml', 'conformance/ci/*.sh', 'conformance/ci/*.mjs', 'tools/*.sh', 'apps/*/scripts/ci/*'], { cwd: PADDOCK, encoding: 'utf8' });
  assert.equal(listed.status, 0, listed.stderr);
  return listed.stdout.split('\n').filter((f) => f && !f.endsWith('.test.mjs') && !['conformance/ci/ci-home.sh', 'conformance/ci/job-isolation.sh'].includes(f));
}

test('no workflow, action or CI script builds a path in the CI home except through job-isolation.sh (so isolation cannot be bypassed)', () => {
  const found = [];
  for (const f of ciFiles()) {
    for (const l of read(join(PADDOCK, f)).split('\n')) {
      if (/^\s*(#|\/\/|description:|rem )/.test(l)) continue;
      if (/\.paddock-ci[\\/]|PADDOCK_CI_HOME|\.debrief-ci|DEBRIEF_CI_HOME|ci-home\.sh/.test(l)) found.push(`${f}: ${l.trim()}`);
    }
  }
  assert.deepEqual(found, [], 'ask `bash conformance/ci/job-isolation.sh path <kind> [<name>]` (a new kind is one line in kept-folders.txt)');
});

test('every kind a CI file asks for is in kept-folders.txt, and every kind there is used', () => {
  const kinds = new Set(read(TABLE).split('\n').filter((l) => l.trim() && !l.startsWith('#')).map((l) => l.trim().split(/\s+/)[0]));
  const used = new Set();
  for (const f of ciFiles()) {
    for (const m of read(join(PADDOCK, f)).matchAll(/job-isolation\.sh"? path ([a-z-]+)/g)) used.add(m[1]);
    for (const m of read(join(PADDOCK, f)).matchAll(/"\$(?:iso|isolation)" path ([a-z-]+)/g)) used.add(m[1]);
  }
  for (const k of used) assert.ok(kinds.has(k), `${k} is asked for but not declared`);
  for (const k of kinds) assert.ok(used.has(k), `${k} is declared but nothing uses it`);
});
