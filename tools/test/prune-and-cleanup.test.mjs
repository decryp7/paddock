import assert from 'node:assert/strict';
import { closeSync, existsSync, ftruncateSync, mkdirSync, openSync, symlinkSync, writeFileSync } from 'node:fs';
import { randomBytes } from 'node:crypto';
import { join } from 'node:path';
import { test } from 'node:test';
import { TOOLS, calls, run, scratch, shim, write } from './helpers.mjs';

const big = (root, path, kb) => {
  mkdirSync(join(root, path), { recursive: true });
  // Random bytes: a file system that compresses (the CI runners' temp folders) makes a run of equal bytes nearly free in `du`.
  writeFileSync(join(root, path, 'blob'), randomBytes(kb * 1024));
};
const prune = (dir, cap) => run('prune-build-folders.sh', [dir, String(cap)], { env: { PADDOCK_PRUNE_UNIT: 'kb' } });

test('prune: under the cap nothing is touched', () => {
  const dir = scratch();
  big(dir, 'debug/incremental', 100);
  const r = prune(dir, 100000);
  assert.equal(r.status, 0);
  assert.ok(existsSync(join(dir, 'debug/incremental/blob')));
});

test('prune: over the cap the incremental caches go first and the rest stays', () => {
  const dir = scratch();
  big(dir, 'debug/incremental', 400);
  big(dir, 'debug/deps', 50);
  const r = prune(dir, 300);
  assert.equal(r.status, 0, r.out);
  assert.match(r.out, /removing incremental caches/);
  assert.ok(!existsSync(join(dir, 'debug/incremental')));
  assert.ok(existsSync(join(dir, 'debug/deps/blob')));
});

// A file system that allocates late (ZFS, btrfs, a compressed or delayed-allocation volume on a runner) reports almost nothing in `du` for
// data written a moment ago. A sparse file is the same thing, made deterministic: 400 KB long, nothing allocated.
test('prune: counts the size of the files, not only the blocks the file system has allocated so far', () => {
  const dir = scratch();
  mkdirSync(join(dir, 'debug/incremental'), { recursive: true });
  const fd = openSync(join(dir, 'debug/incremental/blob'), 'w');
  ftruncateSync(fd, 400 * 1024);
  closeSync(fd);
  big(dir, 'debug/deps', 1);
  const r = prune(dir, 300);
  assert.equal(r.status, 0, r.out);
  assert.match(r.out, /removing incremental caches/);
  assert.ok(!existsSync(join(dir, 'debug/incremental')));
});

test('prune: still over after that, the folder is removed', () => {
  const dir = scratch();
  big(dir, 'debug/incremental', 10);
  big(dir, 'debug/deps', 400);
  const r = prune(dir, 300);
  assert.match(r.out, /still .*removing/);
  assert.ok(!existsSync(dir));
});

test('prune: a missing folder is fine, a symlink is never followed, a bad cap is refused', () => {
  assert.equal(prune(join(scratch(), 'nope'), 1).status, 0);
  const real = scratch();
  big(real, 'incremental', 500);
  const link = join(scratch(), 'link');
  symlinkSync(real, link);
  assert.equal(prune(link, 1).status, 0);
  assert.ok(existsSync(join(real, 'incremental/blob')), 'the target of a link is left alone');
  assert.equal(run('prune-build-folders.sh', [scratch(), 'many']).status, 2);
  assert.equal(run('prune-build-folders.sh', []).status, 2);
});

test('host-cleanup: kills only processes anchored on this job, removes only the prefix in its own temp folder, keeps build outputs unless asked', () => {
  const bin = scratch('paddock-bin-');
  shim(bin, 'pkill');
  const tmp = scratch();
  const cwd = scratch();
  mkdirSync(join(tmp, 'pitwall-abc'));
  mkdirSync(join(tmp, 'pitwall-def'));
  mkdirSync(join(tmp, 'other-job'));
  write(cwd, { 'dist/x': '1', 'app/dist/y': '2' });
  shim(bin, 'ps');
  const env = { PADDOCK_PKILL: join(bin, 'pkill'), PADDOCK_PS: join(bin, 'ps'), RUNNER_OS: 'macOS', PADDOCK_CI_SLOT: 's2', PADDOCK_JOB_TEMP: tmp, PADDOCK_BUILD_FOLDER: '/ci/target-pitwall-app-s2' };
  const r = run('host-cleanup.sh', ['--id', 'pitwall', '--process-pattern', '{build}/debug/pitwall-server', '--process-pattern', 'fake[-_]engine', '--process-pattern', '[/\\\\]test-browser-pitwall-{slot}', '--process-pattern', 'chrome-{slot}', '--process-pattern', '{tmp}/x'], { bin, cwd, env });
  assert.equal(r.status, 0, r.out);
  const re = (p) => p.replaceAll('/', '[/\\\\]').replaceAll('.', '\\.');
  assert.deepEqual(calls(bin).filter((c) => c.startsWith('pkill')), [`pkill -f ${re('/ci/target-pitwall-app-s2')}/debug/pitwall-server`, 'pkill -f [/\\\\]test-browser-pitwall-s2([^0-9]|$)', `pkill -f ${re(tmp)}/x`], 'each pattern scoped to this job');
  assert.match(r.out, /skipped the process pattern 'chrome-\{slot\}'/, '{slot} must follow a kept folder\'s name');
  assert.match(r.out, /skipped the process pattern 'fake\[-_\]engine': it is not anchored/, 'an unscoped pattern could kill another job\'s processes');
  assert.ok(!existsSync(join(tmp, 'pitwall-abc')) && !existsSync(join(tmp, 'pitwall-def')));
  assert.ok(existsSync(join(tmp, 'other-job')), 'other folders stay');
  assert.ok(existsSync(join(cwd, 'dist/x')), 'build outputs stay without --build-outputs');
  const target = scratch();
  big(target, 'dist', 1);
  big(target, 'debug', 1);
  big(target, 'release', 1);
  run('host-cleanup.sh', ['--id', 'pitwall', '--build-outputs'], { bin, cwd, env: { ...env, CARGO_TARGET_DIR: target } });
  assert.ok(!existsSync(join(cwd, 'dist')) && !existsSync(join(cwd, 'app/dist')));
  assert.ok(!existsSync(join(target, 'dist')) && !existsSync(join(target, 'debug')));
  assert.ok(existsSync(join(target, 'release/blob')), 'only dist and debug of the target folder');
});

test('host-cleanup: the leak check names what still runs after the clean-up; on Windows PowerShell ends and lists processes (Git bash has no pkill)', () => {
  const bin = scratch('paddock-bin-');
  shim(bin, 'pkill');
  // ps lists a survivor of this job, another slot's process of the same name, and an unrelated one.
  shim(bin, 'ps', 'printf "%s\\n" "  41 /ci/target-pitwall-app-s2/debug/pitwall-server --fake-engine" "  42 /ci/target-pitwall-app-s3/debug/pitwall-server --fake-engine" "  43 /usr/bin/other"');
  const env = { PADDOCK_PKILL: join(bin, 'pkill'), PADDOCK_PS: join(bin, 'ps'), RUNNER_OS: 'macOS', PADDOCK_CI_SLOT: 's2', PADDOCK_BUILD_FOLDER: '/ci/target-pitwall-app-s2' };
  const r = run('host-cleanup.sh', ['--id', 'pitwall', '--process-pattern', '{build}[/\\\\]debug[/\\\\]pitwall-server'], { bin, env });
  assert.equal(r.status, 0, r.out);
  assert.match(r.out, /::warning::host-cleanup: processes of this job are still running after the clean-up/);
  assert.match(r.out, /^ {2} {2}41 \/ci\/target-pitwall-app-s2\/debug\/pitwall-server --fake-engine$/m);
  assert.doesNotMatch(r.out, /\b42 |\b43 /, 'only this job\'s processes');
  // Windows: no pkill; PowerShell gets the scoped pattern in its environment, ends the matches, then lists the survivors.
  const win = scratch('paddock-bin-');
  shim(win, 'pkill');
  shim(win, 'powershell.exe', `echo "pattern=$PADDOCK_KILL_PATTERN" >> "${win}/calls.log"; case "$*" in *Stop-Process*) ;; *) printf '%s\\r\\n' '77 C:\\ci\\target-pitwall-app-s2\\debug\\pitwall-server.exe' ;; esac`);
  const rw = run('host-cleanup.sh', ['--id', 'pitwall', '--process-pattern', '{build}[/\\\\]debug[/\\\\]pitwall-server'], { bin: win, env: { RUNNER_OS: 'Windows', PADDOCK_POWERSHELL: join(win, 'powershell.exe'), PADDOCK_PKILL: join(win, 'pkill'), PADDOCK_CI_SLOT: 's2', PADDOCK_BUILD_FOLDER: 'C:/ci/target-pitwall-app-s2' } });
  assert.equal(rw.status, 0, rw.out);
  const log = calls(win);
  assert.ok(!log.some((c) => c.startsWith('pkill')), 'pkill is not used on Windows');
  assert.equal(log.filter((c) => c.startsWith('powershell.exe') && /Stop-Process/.test(c)).length, 1, 'ends the matches');
  assert.ok(log.includes('pattern=[/\\\\]ci[/\\\\]target-pitwall-app-s2[/\\\\]debug[/\\\\]pitwall-server'), `the drive-less, separator-agnostic pattern: ${log}`);
  assert.match(rw.out, /^ {2}77 C:\\ci\\target-pitwall-app-s2\\debug\\pitwall-server\.exe$/m, 'the survivor, without the carriage return');
});

test('host-cleanup: without a temp folder of its own the sweep leaves the shared temp alone; exact process names are gone', () => {
  const bin = scratch('paddock-bin-');
  shim(bin, 'pkill');
  const shared = scratch();
  mkdirSync(join(shared, 'pitwall-it-other-job'));
  const r = run('host-cleanup.sh', ['--id', 'pitwall', '--temp-prefix', 'pitwall-it-'], { bin, env: { PADDOCK_PKILL: join(bin, 'pkill'), RUNNER_OS: 'macOS', TMPDIR: shared } });
  assert.equal(r.status, 0, r.out);
  assert.ok(existsSync(join(shared, 'pitwall-it-other-job')), 'a concurrent job\'s test folder in the shared temp stays');
  assert.match(r.out, /no temp folder of this job/);
  assert.equal(run('host-cleanup.sh', ['--id', 'pitwall', '--process', 'pitwall-server'], { bin, env: { PADDOCK_PKILL: join(bin, 'pkill') } }).status, 2, 'an exact name would match every job');
});

test('host-cleanup: a bad id or prefix is refused before anything is removed; a custom prefix is honoured', () => {
  const bin = scratch('paddock-bin-');
  shim(bin, 'pkill');
  const tmp = scratch();
  mkdirSync(join(tmp, 'keep-me'));
  mkdirSync(join(tmp, 'job-1'));
  const env = { PADDOCK_PKILL: join(bin, 'pkill'), RUNNER_OS: 'macOS', PADDOCK_JOB_TEMP: tmp };
  assert.equal(run('host-cleanup.sh', [], { bin, env }).status, 2);
  assert.equal(run('host-cleanup.sh', ['--id', 'Bad Id'], { bin, env }).status, 2);
  assert.equal(run('host-cleanup.sh', ['--id', 'x', '--temp-prefix', '../'], { bin, env }).status, 2);
  assert.equal(run('host-cleanup.sh', ['--id', 'x', '--temp-prefix', 'job-'], { bin, env }).status, 0);
  assert.ok(existsSync(join(tmp, 'keep-me')) && !existsSync(join(tmp, 'job-1')));
  assert.deepEqual(calls(bin), []);
});

// prune-ci-build-folder.sh: only a target-* folder directly in the runner's CI home is ever capped.
import { spawnSync } from 'node:child_process';
import { realpathSync } from 'node:fs';
function ciPrune(dir, home, extra = [], slot = 's1') {
  const r = spawnSync('bash', [join(TOOLS, 'prune-ci-build-folder.sh'), dir, '1', ...extra], {
    env: { PATH: '/usr/bin:/bin', HOME: home, RUNNER_OS: 'macOS', PADDOCK_CI_SLOT: slot, PADDOCK_PRUNE_UNIT: 'kb' },
    encoding: 'utf8',
  });
  return { status: r.status, out: r.stdout + r.stderr };
}
function ciHome() {
  const home = realpathSync(scratch('paddock-prune-home-'));
  mkdirSync(join(home, '.paddock-ci'), { recursive: true });
  return home;
}

test('prune-ci: a target-*-<slot> folder of this job slot in the CI home is capped (incremental caches go, bundles with --drop-bundles)', () => {
  const home = ciHome();
  const dir = join(home, '.paddock-ci', 'target-app-app-s1');
  big(dir, 'debug/incremental', 3000);
  big(dir, 'dist/bundle', 10);
  const r = ciPrune(dir, home, ['--drop-bundles']);
  assert.equal(r.status, 0, r.out);
  assert.ok(!existsSync(join(dir, 'debug/incremental')), r.out);
  assert.ok(!existsSync(join(dir, 'dist/bundle')));
  // A concurrent job's folder (another slot) is never pruned, even by name.
  const theirs = join(home, '.paddock-ci', 'target-app-app-s2');
  big(theirs, 'debug/incremental', 3000);
  const r2 = ciPrune(theirs, home);
  assert.match(r2.out, /leaving it alone/);
  assert.ok(existsSync(join(theirs, 'debug/incremental/blob')), 'the other slot\'s build folder stays');
  assert.match(ciPrune(dir, home, [], '').out, /leaving it alone/, 'no slot, nothing pruned');
});

test('prune-ci: anything else is left alone with a notice (unset, $HOME, /, outside the CI home, a symlink, not target-*, the CI home itself)', () => {
  const home = ciHome();
  const outside = realpathSync(scratch('paddock-prune-outside-'));
  big(outside, 'target-x/debug/incremental', 3000);
  const real = join(home, '.paddock-ci', 'target-real-s1');
  big(real, 'debug/incremental', 3000);
  symlinkSync(real, join(home, '.paddock-ci', 'target-link'));
  big(home, '.paddock-ci/notes/debug/incremental', 3000);
  big(home, 'Documents/debug/incremental', 3000);
  const cases = {
    unset: ['', join(real, 'debug/incremental/blob')],
    home: [home, join(home, 'Documents/debug/incremental/blob')],
    root: ['/', join(home, 'Documents/debug/incremental/blob')],
    outside: [join(outside, 'target-x'), join(outside, 'target-x/debug/incremental/blob')],
    symlink: [join(home, '.paddock-ci', 'target-link'), join(real, 'debug/incremental/blob')],
    'not target-*': [join(home, '.paddock-ci', 'notes'), join(home, '.paddock-ci/notes/debug/incremental/blob')],
    'the CI home': [join(home, '.paddock-ci'), join(real, 'debug/incremental/blob')],
    'a dotted path out of it': [join(home, '.paddock-ci', 'target-real-s1', '..', 'target-real-s1', '..', '..', 'Documents'), join(home, 'Documents/debug/incremental/blob')],
  };
  for (const [name, [dir, canary]] of Object.entries(cases)) {
    const r = ciPrune(dir, home);
    assert.equal(r.status, 0, `${name}: ${r.out}`);
    assert.ok(existsSync(canary), `${name} must not delete ${canary}`);
    assert.match(r.out, /nothing to prune|leaving it alone/, name);
  }
});
