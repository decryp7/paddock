// Job isolation (.gitea/actions/README.md, "Job isolation"; ADR 0025): conformance/ci/job-isolation.sh with a fake HOME, fake
// runner folders and a fake clock (never the real CI home or runner), and the contract that every host job takes a slot and
// that no CI file reaches a CI-home folder except through the script.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, utimesSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { parseJobs, parseSteps } from '../src/util/workflows.mjs';
import { PADDOCK } from './helpers.mjs';

const SCRIPT = join(PADDOCK, 'conformance', 'ci', 'job-isolation.sh');
const TABLE = join(PADDOCK, 'conformance', 'ci', 'kept-folders.txt');
const read = (p) => readFileSync(p, 'utf8');

/**
 * A stand-in for `./dev ci slot` (paddock-dev), for these sequential tests of the shell wiring: a slot is a folder made with
 * mkdir, and a held slot whose owner folder is gone is free again (the real holder ends when the job folder goes; the real
 * lock is the kernel's, tested with racing processes in tools/paddock-dev/tests/slots.rs and crates/paddock-slots). It logs
 * every call to <root>/dev.log.
 */
const FAKE_DEV = `#!/usr/bin/env bash
set -u
echo "$*" >> "$FAKE_DEV_LOG"
[ "$1 $2" = "ci slot" ] || exit 2
cmd=$3; shift 3
max= owner= root= slot= token= run=()
while [ $# -gt 0 ]; do
  case $1 in --max) max=$2; shift ;; --owner) owner=$2; shift ;; --root) root=$2; shift ;; --slot) slot=$2; shift ;; --token) token=$2; shift ;;
    --) shift; run=("$@"); break ;; esac
  shift
done
d="$root/slots"; mkdir -p "$d"
held() { [ -d "$d/$1.lock" ] && { [ -n "$(sed -n 's/^owner=//p' "$d/$1.owner" 2>/dev/null)" ] || [ -f "$d/$1.idle" ]; } && { [ -f "$d/$1.idle" ] || [ -d "$(sed -n 's/^owner=//p' "$d/$1.owner")" ]; }; }
take() { if held "$1"; then return 1; fi; rm -rf "$d/$1.lock" "$d/$1.owner" "$d/$1.idle"; mkdir "$d/$1.lock"; }
case $cmd in
  acquire)
    for n in $(seq 1 "$max"); do
      if take "s$n"; then printf 'pid=1\\ntoken=t%s\\nowner=%s\\n' "$n$$" "$owner" > "$d/s$n.owner"; echo "s$n t$n$$"; exit 0; fi
    done
    echo "error: All $max slots are busy." >&2; exit 4 ;;
  release)
    if [ "$(sed -n 's/^token=//p' "$d/$slot.owner" 2>/dev/null)" = "$token" ]; then rm -rf "$d/$slot.lock" "$d/$slot.owner"; fi
    echo "$slot released" ;;
  status)
    for n in $(seq 1 "$max"); do if held "s$n"; then echo "s$n busy"; else echo "s$n free"; fi; done ;;
  idle-run)
    if ! take "$slot"; then echo "$slot busy: skipped"; exit 0; fi
    touch "$d/$slot.idle"
    PADDOCK_CI_IDLE_SLOT=$slot "\${run[@]}"; rc=$?
    rm -rf "$d/$slot.lock" "$d/$slot.idle"
    [ $rc = 0 ] && echo "$slot idle: ran" || exit 1 ;;
esac
`;

/** A fake host: a HOME (the CI home is its .paddock-ci) and a runner work folder with per-job folders. */
function host() {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'paddock-isolation-')));
  mkdirSync(join(root, 'home'));
  const dev = join(root, 'dev');
  writeFileSync(dev, FAKE_DEV);
  chmodSync(dev, 0o755);
  const log = join(root, 'dev.log');
  writeFileSync(log, '');
  // The disk is faked too (ER-14): a df that reports 1000 GB free, and no free-disk floor, so acquire never reads or
  // depends on the real disk of the machine running the tests (a Mac with 17 GB free failed eight tests). The floor tests
  // set both explicitly.
  // The jobs' socket folders (job-tmpdir.sh) under the fake host, never the real /tmp.
  const sockets = join(root, 'sockets');
  mkdirSync(sockets);
  const df = join(root, 'df-ample');
  writeFileSync(df, '#!/bin/sh\necho "Filesystem 1024-blocks Used Available Capacity Mounted"\necho "disk 1 1 $((1000 * 1048576)) 1% /"\n');
  chmodSync(df, 0o755);
  /** A job as the runner starts it: <work>/<16 hex>/tmp is its RUNNER_TEMP, and it has its own GITHUB_ENV. */
  const job = (id, extra = {}) => {
    const temp = join(root, 'work', id, 'tmp');
    mkdirSync(temp, { recursive: true });
    const envFile = join(root, 'work', id, 'github-env');
    writeFileSync(envFile, '');
    const base = { PATH: '/usr/bin:/bin', HOME: join(root, 'home'), RUNNER_OS: 'macOS', RUNNER_TEMP: temp, GITHUB_ENV: envFile, GITHUB_RUN_ID: '7', GITHUB_JOB: id, PADDOCK_DEV: dev, FAKE_DEV_LOG: log, PADDOCK_SLOT_WAIT_TRIES: '0', PADDOCK_DF: df, PADDOCK_CI_MIN_FREE_GB: '0', PADDOCK_SOCKET_ROOT: sockets, ...extra };
    const exec = (args, more = {}) => spawnSync('bash', [SCRIPT, ...args], { env: { ...base, ...more }, encoding: 'utf8' });
    const exported = () => Object.fromEntries(read(envFile).split('\n').filter(Boolean).map((l) => [l.slice(0, l.indexOf('=')), l.slice(l.indexOf('=') + 1)]));
    /** Runs a command with what acquire exported, as the job's later steps see it. */
    const later = (args, more = {}) => exec(args, { ...exported(), ...more });
    return { id, temp, exec, exported, later, gone: () => rmSync(join(root, 'work', id), { recursive: true, force: true }) };
  };
  return { root, ci: join(root, 'home', '.paddock-ci'), job, log: () => read(log) };
}

test('two jobs on one host get different slots from the OS-lock holder, and each points its temp folders at the runner\'s own folder for it', () => {
  const h = host();
  const a = h.job('aaaaaaaaaaaaaaaa');
  const b = h.job('bbbbbbbbbbbbbbbb');
  assert.equal(a.exec(['acquire']).status, 0);
  const rb = b.exec(['acquire']);
  assert.equal(rb.status, 0, rb.stderr + rb.stdout);
  assert.equal(a.exported().PADDOCK_CI_SLOT, 's1');
  assert.equal(b.exported().PADDOCK_CI_SLOT, 's2');
  assert.match(a.exported().PADDOCK_CI_SLOT_TOKEN, /^t1\d+$/, 'the holder\'s token, for the release');
  assert.match(h.log(), new RegExp(`^ci slot acquire --max 4 --owner ${a.temp} --root ${h.ci}$`, 'm'), 'the holder watches the runner\'s job folder; the slots live in the CI home');
  assert.equal(a.exported().TMPDIR, a.temp, 'TMPDIR is the runner\'s per-job folder');
  assert.equal(b.exported().PADDOCK_JOB_TEMP, b.temp);
  const sock = a.exported().PADDOCK_SOCKET_BASE;
  assert.ok(sock && sock.startsWith(join(h.root, 'sockets', 'pd-')) && existsSync(sock), 'a short socket folder of the job (job-tmpdir.sh)');
  assert.notEqual(sock, b.exported().PADDOCK_SOCKET_BASE, 'one per job');
  assert.notEqual(a.later(['path', 'target', 'x-app']).stdout, b.later(['path', 'target', 'x-app']).stdout, 'the same name, two folders');
  assert.equal(a.later(['path', 'target', 'x-app']).stdout.trim(), join(h.ci, 'target-x-app-s1'));
  assert.equal(b.later(['path', 'src', 'x-app-macOS']).stdout.trim(), join(h.ci, 'src', 'x-app-macOS-s2'));
  assert.equal(a.later(['path', 'tools']).stdout.trim(), b.later(['path', 'tools']).stdout.trim(), 'a shared store is one folder for every slot');
  assert.notEqual(a.later(['path', 'npm']).stdout.trim(), b.later(['path', 'npm']).stdout.trim(), 'npm\'s cache is the slot\'s own');
  assert.equal(a.later(['acquire']).status, 0, 'a second acquire in the same job keeps its slot');
  assert.equal(a.exported().PADDOCK_CI_SLOT, 's1');
  assert.equal(a.later(['release']).status, 0);
  assert.ok(!existsSync(sock), 'release removes the job\'s socket folder');
  assert.ok(existsSync(b.exported().PADDOCK_SOCKET_BASE), 'and only its own');
});

test('the slot count follows PADDOCK_CI_SLOTS (the pool maximum); a full house waits, then fails and names the holders', () => {
  const h = host();
  const jobs = ['1', '2', '3', '4'].map((n) => h.job(n.repeat(16), { PADDOCK_CI_SLOTS: '3' }));
  for (const j of jobs.slice(0, 3)) assert.equal(j.exec(['acquire']).status, 0);
  assert.deepEqual(jobs.slice(0, 3).map((j) => j.exported().PADDOCK_CI_SLOT), ['s1', 's2', 's3']);
  const full = jobs[3].exec(['acquire'], { PADDOCK_SLOT_WAIT_TRIES: '2', PADDOCK_SLOT_WAIT_SECONDS: '0' });
  assert.notEqual(full.status, 0);
  assert.match(full.stderr, /all 3 slots are held; trying again \(2 of 2\)/);
  assert.match(full.stderr, /no slot \(paddock-dev exit 4\)/);
  assert.match(full.stderr, new RegExp(`s2 busy: job folder ${jobs[1].temp}`));
  assert.equal(jobs[1].later(['release']).status, 0);
  assert.equal(jobs[3].exec(['acquire']).status, 0, 'a released slot is free again');
  assert.equal(jobs[3].exported().PADDOCK_CI_SLOT, 's2', 'the lowest free slot: its kept folders are the warmest');
  const bad = h.job('5'.repeat(16), { PADDOCK_CI_SLOTS: 'many' }).exec(['acquire']);
  assert.notEqual(bad.status, 0);
  assert.match(read(SCRIPT), /PADDOCK_CI_SLOT=.*\n(?:.*\n)*?.*PADDOCK_CI_SLOT_TOKEN=/, 'the token goes to the later steps');
  assert.doesNotMatch(read(SCRIPT), /stale\(|\.stale-|PADDOCK_SLOT_MAX_AGE_MIN|mkdir "\$lock"/, 'no shell lock and no reclaim: the kernel frees a dead holder\'s lock');
});

test('a job that ends without its release frees its slot (the holder ends with the job folder); a release whose holder died fails loudly', () => {
  const h = host();
  const a = h.job('a'.repeat(16), { PADDOCK_CI_SLOTS: '1' });
  assert.equal(a.exec(['acquire']).status, 0);
  const b = h.job('b'.repeat(16), { PADDOCK_CI_SLOTS: '1' });
  assert.notEqual(b.exec(['acquire']).status, 0, 'a live job\'s slot is never taken');
  a.gone(); // the runner deleted the job's folder: the job ended without its release step (cancelled, runner restarted)
  const rb = b.exec(['acquire']);
  assert.equal(rb.status, 0, rb.stdout + rb.stderr);
  assert.equal(b.exported().PADDOCK_CI_SLOT, 's1');
  // b's holder is gone while b still runs (killed with the job's processes): the release says so and fails the job.
  rmSync(join(h.ci, 'slots', 's1.owner'));
  const r = b.later(['release']);
  assert.notEqual(r.status, 0);
  assert.match(r.stderr, /the holder of s1 ended before the job did .* isolation was not guaranteed/);
});

test('release frees only the job\'s own slot; a job without one, or in a Linux container, does nothing', () => {
  const h = host();
  const a = h.job('a'.repeat(16));
  a.exec(['acquire']);
  const b = h.job('b'.repeat(16));
  const stolen = b.exec(['release'], { PADDOCK_CI_SLOT: 's1', PADDOCK_CI_SLOT_TOKEN: 'not-a-s-token' });
  assert.notEqual(stolen.status, 0, 'a slot that is not this job\'s is reported');
  assert.ok(existsSync(join(h.ci, 'slots', 's1.owner')), 'and stays with its holder');
  assert.equal(b.exec(['release']).status, 0, 'no slot: nothing to release');
  const r = a.later(['release']);
  assert.equal(r.status, 0, r.stdout);
  assert.ok(!existsSync(join(h.ci, 'slots', 's1.owner')));
  assert.match(h.log(), /^ci slot release --slot s1 --token t1\d+ --root /m);
  const linux = h.job('l'.repeat(16), { RUNNER_OS: 'Linux' });
  assert.equal(linux.exec(['acquire']).status, 0);
  assert.equal(read(join(h.root, 'work', 'l'.repeat(16), 'github-env')), '', 'a container exports nothing');
  assert.equal(linux.exec(['path', 'target', 'x']).stdout.trim(), join(h.ci, 'target-x-s1'), 'a container is the single slot s1');
  assert.match(a.exec(['status']).stdout, /s1 free/);
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
  assert.ok(existsSync(join(disk, 'slots', 's1.owner')), 'the slots live in the relocated home');
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
  assert.equal(a.later(['port', 'sccache']).stdout.trim(), '4227');
  assert.equal(a.later(['port', 'webkit'], { PADDOCK_CI_SLOT: 's3' }).stdout.trim(), '4520');
  assert.notEqual(a.later(['port', 'nope']).status, 0, 'only declared port blocks');
  assert.notEqual(a.later(['pattern', 'gitea-runner']).status, 0, 'a pattern that names no folder of the job is refused');
});

test('{slot} in a process pattern ends it, right after a kept folder\'s name, and never matches a longer slot number', () => {
  const h = host();
  const a = h.job('a'.repeat(16));
  a.exec(['acquire']);
  const p = a.later(['pattern', '[/\\\\]test-browser-spotter-{slot}']);
  assert.equal(p.status, 0, p.stderr);
  const re = new RegExp(p.stdout.trim());
  for (const line of ['/ci/test-browser-spotter-s1/chrome --headless', 'C:\\ci\\test-browser-spotter-s1\\chrome.exe', '/ci/test-browser-spotter-s1']) assert.match(line, re);
  for (const line of ['/ci/test-browser-spotter-s12/chrome', '/ci/test-browser-spotter-s2/chrome', '/ci/test-browser-spotter/chrome']) assert.doesNotMatch(line, re);
  assert.match(a.later(['pattern', '[/\\\\]src[/\\\\]paddock-crate-macOS-{slot}']).stdout, /src\[\/\\\\\]paddock-crate-macOS-s1/);
  for (const bad of ['chrome-{slot}', '{tmp}/x-{slot}', '[/\\\\]test-browser-spotter-{slot}[/\\\\]', '[/\\\\]target-x-{slot}-{slot}', 'xtarget-x-{slot}']) {
    assert.notEqual(a.later(['pattern', bad]).status, 0, `${bad} is not anchored on a kept folder of the slot`);
  }
});

test('disk: the first s1 job takes over a kept folder of the old layout; idle slots above PADDOCK_CI_KEEP_SLOTS are swept, a held one never', () => {
  const h = host();
  mkdirSync(join(h.ci, 'target-x-app', 'debug'), { recursive: true });
  writeFileSync(join(h.ci, 'target-x-app', 'debug', 'warm'), '');
  mkdirSync(join(h.ci, 'src', 'x-app-macOS'), { recursive: true });
  const keep2 = { PADDOCK_CI_KEEP_SLOTS: '2' };
  const a = h.job('a'.repeat(16), keep2);
  a.exec(['acquire']);
  assert.equal(a.later(['path', 'target', 'x-app']).stdout.trim(), join(h.ci, 'target-x-app-s1'));
  assert.ok(existsSync(join(h.ci, 'target-x-app-s1', 'debug', 'warm')), 'renamed, not copied: warm and no second copy');
  assert.ok(!existsSync(join(h.ci, 'target-x-app')));
  a.later(['path', 'src', 'x-app-macOS']);
  assert.ok(!existsSync(join(h.ci, 'src', 'x-app-macOS')) && existsSync(join(h.ci, 'src', 'x-app-macOS-s1')));
  // A third concurrent job (slot s3, above the two kept slots): its folders go when it releases, under s3's lock.
  const b = h.job('b'.repeat(16), keep2);
  const c = h.job('c'.repeat(16), keep2);
  b.exec(['acquire']);
  c.exec(['acquire']);
  assert.equal(c.exported().PADDOCK_CI_SLOT, 's3');
  const third = c.later(['path', 'target', 'x-app']).stdout.trim();
  const secondFolder = b.later(['path', 'target', 'x-app']).stdout.trim();
  const rc = c.later(['release']);
  assert.equal(rc.status, 0, rc.stdout + rc.stderr);
  assert.ok(!existsSync(third), 'slot s3 is above the kept slots: its folders are removed with it');
  assert.ok(existsSync(secondFolder), 'slot s2 is kept');
  assert.match(h.log(), /^ci slot idle-run --slot s3 --root \S+ -- bash \S+job-isolation\.sh sweep-slot$/m, 'deleted only under s3\'s lock');
  // A held slot above the limit is never swept: its job still builds in it.
  const d = h.job('d'.repeat(16), { PADDOCK_CI_KEEP_SLOTS: '1' });
  assert.equal(d.exec(['acquire']).status, 0);
  assert.ok(existsSync(secondFolder), 's2 is held by a running job: its folders stay');
  // Folders of an idle slot above the limit (its job died without releasing): swept at the next acquire.
  mkdirSync(join(h.ci, 'test-browser-y-s7'), { recursive: true });
  mkdirSync(join(h.ci, 'test-browser-y-s1x'), { recursive: true });
  h.job('e'.repeat(16), keep2).exec(['acquire']);
  assert.ok(!existsSync(join(h.ci, 'test-browser-y-s7')));
  assert.ok(existsSync(join(h.ci, 'test-browser-y-s1x')), 'only exact slot folders');
  assert.ok(existsSync(join(h.ci, 'target-x-app-s1')) && existsSync(secondFolder));
  // sweep-slot outside idle-run (no lock held) refuses to delete anything.
  const bare = a.later(['sweep-slot', 'all']);
  assert.notEqual(bare.status, 0);
  assert.match(bare.stderr, /only under `dev ci slot idle-run`/);
});

test('disk: kept folders no job used for PADDOCK_CI_ORPHAN_DAYS (folders no workflow names any more) go when their slot is idle; used ones stay', () => {
  const h = host();
  const daysAgo = (dir, days) => { mkdirSync(dir, { recursive: true }); const t = Date.now() / 1000 - days * 86400; utimesSync(dir, t, t); };
  daysAgo(join(h.ci, 'target-renamed-job-s1'), 20);
  daysAgo(join(h.ci, 'target-old-unslotted'), 20);
  daysAgo(join(h.ci, 'target-recent-unslotted'), 2);
  daysAgo(join(h.ci, 'target-x-app-s1'), 20);
  daysAgo(join(h.ci, 'tools'), 20);
  const a = h.job('a'.repeat(16));
  a.exec(['acquire']);
  assert.ok(existsSync(join(h.ci, 'target-renamed-job-s1')), 'slot s1 is held by this job: nothing of it is swept');
  a.later(['path', 'target', 'x-app']); // in use: its time stamp is renewed
  assert.equal(a.later(['release']).status, 0);
  assert.ok(!existsSync(join(h.ci, 'target-renamed-job-s1')), 'unused for 20 days: gone');
  assert.ok(!existsSync(join(h.ci, 'target-old-unslotted')), 'an old-layout folder nobody took over: gone');
  assert.ok(existsSync(join(h.ci, 'target-recent-unslotted')), 'recent: stays');
  assert.ok(existsSync(join(h.ci, 'target-x-app-s1')), 'used by the job: stays');
  assert.ok(existsSync(join(h.ci, 'tools')), 'shared stores are not swept');
});

test('disk: below the free-disk floor acquire removes idle slots\' kept folders, highest first, never a held slot\'s; still below, it fails and frees its slot', () => {
  const h = host();
  // A df whose free space is 10 GB while target-big-s4 exists, 100 GB after (or always 10 GB with FAKE_FULL).
  const df = join(h.root, 'df');
  writeFileSync(df, `#!/bin/sh\necho "Filesystem 1024-blocks Used Available Capacity Mounted"\nif [ -n "\${FAKE_FULL:-}" ] || [ -d "${join(h.ci, 'target-big-s4')}" ]; then gb=10; else gb=100; fi\necho "disk 1 1 $((gb * 1048576)) 1% /"\n`);
  chmodSync(df, 0o755);
  const env = { PADDOCK_DF: df, PADDOCK_CI_MIN_FREE_GB: '30' };
  const b = h.job('b'.repeat(16), env);
  b.exec(['acquire']); // s1
  const c = h.job('c'.repeat(16), env);
  c.exec(['acquire']); // s2, held while the floor is enforced
  mkdirSync(join(h.ci, 'target-big-s4'), { recursive: true });
  mkdirSync(join(h.ci, 'target-mid-s2'), { recursive: true });
  const a = h.job('a'.repeat(16), env);
  const r = a.exec(['acquire']);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.match(r.stderr, /10 GB free on the CI home's disk, below the floor of 30 GB/);
  assert.equal(a.exported().PADDOCK_CI_SLOT, 's3');
  assert.ok(!existsSync(join(h.ci, 'target-big-s4')), 'the idle slot s4\'s folders went, under its lock');
  assert.ok(existsSync(join(h.ci, 'target-mid-s2')), 'held by a running job: kept');
  const full = h.job('f'.repeat(16), { ...env, FAKE_FULL: '1' });
  const rf = full.exec(['acquire']);
  assert.notEqual(rf.status, 0);
  assert.match(rf.stderr, /still only 10 GB free .* Free disk on the host, then re-run the job/);
  assert.match(a.exec(['status']).stdout, /s4 free/, 'the failed job gave its slot back');
});

test('disk: the floor comes from PADDOCK_CI_MIN_FREE_GB (default 30 GB) and acquire refuses below it, on a faked disk', () => {
  const h = host();
  // A disk that always has 20 GB free, nothing to sweep: the floor alone decides.
  const df = join(h.root, 'df-20');
  writeFileSync(df, '#!/bin/sh\necho "Filesystem 1024-blocks Used Available Capacity Mounted"\necho "disk 1 1 $((20 * 1048576)) 1% /"\n');
  chmodSync(df, 0o755);
  const run = (id, floor) => h.job(id.repeat(16), { PADDOCK_DF: df, PADDOCK_CI_MIN_FREE_GB: floor }).exec(['acquire']);
  const unset = run('a', '');
  assert.notEqual(unset.status, 0, 'no floor given: the default of 30 GB applies');
  assert.match(unset.stderr, /still only 20 GB free .*\(floor 30 GB\)/);
  const above = run('b', '21');
  assert.notEqual(above.status, 0, 'one GB short of the floor refuses');
  assert.match(above.stderr, /still only 20 GB free .*\(floor 21 GB\)/);
  assert.equal(run('c', '20').status, 0, 'exactly at the floor is enough');
  assert.equal(run('d', '0').status, 0);
});

test('acquire sweeps the socket folders of jobs that were killed before their release (under the fake host, never /tmp)', () => {
  const h = host();
  const dead = join(h.root, 'sockets', 'pd-killedjb');
  mkdirSync(dead, { mode: 0o700 });
  writeFileSync(join(dead, '.owner'), `${join(h.root, 'work', 'long-gone', 'tmp')}\n`);
  const a = h.job('a'.repeat(16));
  assert.equal(a.exec(['acquire']).status, 0);
  assert.ok(!existsSync(dead), 'the killed job\'s socket folder is gone');
  assert.ok(existsSync(a.exported().PADDOCK_SOCKET_BASE), 'this job\'s own is made');
});

test('the declared port blocks never overlap each other or an end-to-end port, for every slot up to the default PADDOCK_CI_SLOTS', () => {
  const slots = Number(/PADDOCK_CI_SLOTS:-(\d+)/.exec(read(SCRIPT))[1]);
  const blocks = read(join(PADDOCK, 'conformance', 'ci', 'ports.txt')).split('\n').filter((l) => l.trim() && !l.startsWith('#')).map((l) => l.trim().split(/\s+/));
  const used = new Map();
  const take = (port, who) => {
    assert.ok(!used.has(port), `port ${port}: ${used.get(port)} and ${who}`);
    used.set(port, who);
  };
  for (const [name, base, width, stride] of blocks) {
    for (let n = 1; n <= slots; n += 1) for (let i = 0; i < Number(width); i += 1) take(Number(base) + (n - 1) * Number(stride) + i, `${name} s${n}`);
  }
  // Every app's end-to-end ports (paddock.ci.json) and the workflows' fixed ports.
  for (const app of spawnSync('git', ['ls-files', 'apps/*/paddock.ci.json'], { cwd: PADDOCK, encoding: 'utf8' }).stdout.split('\n').filter(Boolean)) {
    const c = JSON.parse(read(join(PADDOCK, app)));
    for (const line of [...(c.e2e ?? []), ...(c.e2eWebkit ?? [])]) {
      const range = /PORT=\$\(\((\d+) \+ E2E_INDEX\)\)/.exec(line);
      if (range) for (let i = 1; i <= (c.e2eShards ?? 1); i += 1) take(Number(range[1]) + i, `${app} shard ${i}`);
      for (const m of line.matchAll(/PORT=(\d+)/g)) take(Number(m[1]), app);
    }
  }
  for (const f of ['ci.yml', 'debrief.yml']) {
    for (const m of read(join(WORKFLOWS, f)).matchAll(/^\s*\w*PORT: (.+)$/gm)) {
      for (const n of m[1].matchAll(/['"](\d{4})['"]/g)) take(Number(n[1]), `${f} ${m[0].trim()}`);
      for (const n of m[1].matchAll(/"(\d{3})\$\{\{ matrix\.shard \}\}"/g)) for (const sh of [1, 2]) take(Number(`${n[1]}${sh}`), `${f} shard ${sh}`);
    }
  }
  assert.ok(used.size > slots * 4, `counted the ports (${used.size})`);
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

// Two bash 5 differences the Mac's bash 3.2 does not show (CI's containers run bash 5.2): patsub_replacement treats a
// backslash in ${v//p/r}'s replacement as an escape (the separator class [/\\] became [/\]), and command substitutions
// drop -e unless inherit_errexit is set (a missing CI home became an empty one, paths at the root of the disk).
test('job-isolation.sh fills patterns as plain text and keeps -e in command substitutions (bash 5)', () => {
  const text = read(SCRIPT);
  assert.doesNotMatch(text, /\$\{pat\/\/[^}]*\$\(/, 'no ${pat//...} with a computed replacement: use fill');
  assert.match(text, /^shopt -s inherit_errexit 2>\/dev\/null \|\| true/m);
  const h = host();
  const a = h.job('a'.repeat(16));
  a.exec(['acquire']);
  const p = a.later(['pattern', '{build}[/\\\\]x'], { PADDOCK_BUILD_FOLDER: '/ci/target-x-s1' });
  assert.equal(p.stdout.trim(), '[/\\\\]ci[/\\\\]target-x-s1[/\\\\]x');
  const gone = a.later(['path', 'target', 'x'], { PADDOCK_CI_HOME: join(h.root, 'unmounted') });
  assert.notEqual(gone.status, 0);
  assert.equal(gone.stdout, '', 'no path is printed for a home that is not there');
});

// CI run 6731: stable-workspace took `path src` with 2>&1, so the migration's message became part of the path ("invalid
// format", "cannot change to 'job-isolation: moved ...'"). A value command prints exactly its value on stdout; every message
// goes to stderr, and no CI file folds stderr into a captured value.
test('value commands print only their value on stdout (messages go to stderr), and no caller captures 2>&1', () => {
  const h = host();
  mkdirSync(join(h.ci, 'src', 'x-app-macOS'), { recursive: true });
  const a = h.job('a'.repeat(16));
  const acq = a.exec(['acquire']);
  assert.equal(acq.stdout, '', `acquire prints messages to stderr only: ${acq.stdout}`);
  const moved = a.later(['path', 'src', 'x-app-macOS']);
  assert.equal(moved.stdout, `${join(h.ci, 'src', 'x-app-macOS-s1')}\n`, 'the migration case: the path and nothing else');
  assert.match(moved.stderr, /moved src\/x-app-macOS to src\/x-app-macOS-s1/);
  for (const args of [['path', 'tools'], ['port', 'sccache'], ['temp'], ['slots']]) assert.equal(a.later(args).stdout.trim().split('\n').length, 1, args.join(' '));
  const rel = a.later(['release']);
  assert.equal(rel.stdout, '', `release prints messages to stderr only: ${rel.stdout}`);
  // sweep-slot (under idle-run, which sends its stdout nowhere): the removal is reported on stderr, where the log keeps it.
  mkdirSync(join(h.ci, 'target-x-app-s7'), { recursive: true });
  const swept = a.exec(['sweep-slot'], { PADDOCK_CI_IDLE_SLOT: 's7' });
  assert.equal(swept.status, 0, swept.stderr);
  assert.equal(swept.stdout, '', `sweep-slot prints messages to stderr only: ${swept.stdout}`);
  assert.match(swept.stderr, /job-isolation: removed target-x-app-s7 \(slot s7 is above the 4 kept slots\)/);
  const found = [];
  for (const f of spawnSync('git', ['ls-files', '.gitea/workflows/*.yml', '.gitea/actions/*/action.yml', 'conformance/ci/*.sh', 'tools/*.sh', 'apps/*/scripts/ci/*'], { cwd: PADDOCK, encoding: 'utf8' }).stdout.split('\n').filter(Boolean)) {
    for (const l of read(join(PADDOCK, f)).split('\n')) if (/\$\([^)]*job-isolation\.sh[^)]*2>&1/.test(l)) found.push(`${f}: ${l.trim()}`);
  }
  assert.deepEqual(found, [], 'never fold the script\'s stderr into a captured value');
});

// Review P2-3: the slot holder's command line names the job's temp folder (--owner) and the CI home (--root); a clean-up
// pattern that matches it would end the holder and free the slot in the middle of the job.
test('a process pattern that would match the slot holder is refused; the job\'s own test processes still match', () => {
  const h = host();
  const a = h.job('a'.repeat(16));
  a.exec(['acquire']);
  for (const bad of ['{tmp}', '{tmp}.*', '{tmp}( |$)']) {
    const r = a.later(['pattern', bad]);
    assert.notEqual(r.status, 0, `${bad} would end the holder`);
    assert.match(r.stderr, /would also match this job's slot holder/);
    assert.equal(r.stdout, '');
  }
  for (const win of [{ PADDOCK_JOB_TEMP: 'C:/w/0123456789abcdef/tmp', RUNNER_OS: 'Windows' }]) {
    assert.notEqual(a.later(['pattern', '{tmp}'], win).status, 0, 'the Windows spelling too');
  }
  const ok = a.later(['pattern', '{tmp}[/\\\\]dyno-it-plugins-[A-Za-z0-9]+']);
  assert.equal(ok.status, 0, ok.stderr);
});

// Review P2-2: a sweep with nothing due never takes a slot's lock, so a new job is never pushed to a colder slot by it.
test('the sweep locks a slot only when something of it is due', () => {
  const h = host();
  mkdirSync(join(h.ci, 'target-fresh-s2'), { recursive: true });
  mkdirSync(join(h.ci, 'target-fresh-s3'), { recursive: true });
  const a = h.job('a'.repeat(16));
  a.exec(['acquire']);
  assert.doesNotMatch(h.log(), /idle-run/, 'recent folders of kept slots: nothing due, no lock taken');
  const t = Date.now() / 1000 - 30 * 86400;
  utimesSync(join(h.ci, 'target-fresh-s3'), t, t);
  h.job('b'.repeat(16)).exec(['acquire']);
  assert.match(h.log(), /^ci slot idle-run --slot s3 /m, 'an aged folder is due');
  assert.doesNotMatch(h.log(), /idle-run --slot s2 /, 's2 still had nothing due');
  assert.ok(!existsSync(join(h.ci, 'target-fresh-s3')) && existsSync(join(h.ci, 'target-fresh-s2')));
});

test('the tables read the same with Windows line ends (a checkout there turns kept-folders.txt and ports.txt into CRLF)', () => {
  const dir = mkdtempSync(join(tmpdir(), 'paddock-crlf-'));
  try {
    const ci = join(dir, 'ci');
    mkdirSync(ci);
    for (const f of ['job-isolation.sh', 'ci-home.sh', 'job-tmpdir.sh', 'kept-folders.txt', 'ports.txt']) {
      const p = join(PADDOCK, 'conformance', 'ci', f);
      if (existsSync(p)) writeFileSync(join(ci, f), f.endsWith('.txt') ? read(p).replace(/\r?\n/g, '\r\n') : read(p));
    }
    const env = { PATH: '/usr/bin:/bin', HOME: dir, RUNNER_OS: 'Windows', PADDOCK_CI_SLOT: 's1' };
    const run = (...args) => spawnSync('bash', [join(ci, 'job-isolation.sh'), ...args], { env, encoding: 'utf8' });
    const p = run('pattern', '[/\\\\]test-browser-spotter-{slot}');
    assert.equal(p.status, 0, `a {slot} pattern is refused with CRLF tables: ${p.stderr}`);
    assert.match(p.stdout, /test-browser-spotter-s1/);
    assert.equal(run('port', 'sccache').stdout.trim(), '4227', 'a port block with CRLF tables');
    assert.equal(run('path', 'target', 'x-app').status, 0, 'a kept folder with CRLF tables');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
