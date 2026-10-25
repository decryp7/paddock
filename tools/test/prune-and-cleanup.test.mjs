import assert from 'node:assert/strict';
import { closeSync, existsSync, ftruncateSync, mkdirSync, openSync, symlinkSync, writeFileSync } from 'node:fs';
import { randomBytes } from 'node:crypto';
import { join } from 'node:path';
import { test } from 'node:test';
import { calls, run, scratch, shim, write } from './helpers.mjs';

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

test('host-cleanup: kills only the names given, removes only the prefix in TMPDIR, keeps build outputs unless asked', () => {
  const bin = scratch('paddock-bin-');
  shim(bin, 'pkill');
  const tmp = scratch();
  const cwd = scratch();
  mkdirSync(join(tmp, 'pitwall-abc'));
  mkdirSync(join(tmp, 'pitwall-def'));
  mkdirSync(join(tmp, 'other-job'));
  write(cwd, { 'dist/x': '1', 'app/dist/y': '2' });
  const env = { PADDOCK_PKILL: join(bin, 'pkill'), TMPDIR: tmp };
  const r = run('host-cleanup.sh', ['--id', 'pitwall', '--process', 'pitwall-server', '--process', 'llama-server', '--process-pattern', 'fake[-_]engine'], { bin, cwd, env });
  assert.equal(r.status, 0, r.out);
  assert.deepEqual(calls(bin), ['pkill -x pitwall-server', 'pkill -x llama-server', 'pkill -f fake[-_]engine']);
  assert.ok(!existsSync(join(tmp, 'pitwall-abc')) && !existsSync(join(tmp, 'pitwall-def')));
  assert.ok(existsSync(join(tmp, 'other-job')), 'other jobs\' folders stay');
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

test('host-cleanup: a bad id or prefix is refused before anything is removed; a custom prefix is honoured', () => {
  const bin = scratch('paddock-bin-');
  shim(bin, 'pkill');
  const tmp = scratch();
  mkdirSync(join(tmp, 'keep-me'));
  mkdirSync(join(tmp, 'job-1'));
  const env = { PADDOCK_PKILL: join(bin, 'pkill'), TMPDIR: tmp };
  assert.equal(run('host-cleanup.sh', [], { bin, env }).status, 2);
  assert.equal(run('host-cleanup.sh', ['--id', 'Bad Id'], { bin, env }).status, 2);
  assert.equal(run('host-cleanup.sh', ['--id', 'x', '--temp-prefix', '../'], { bin, env }).status, 2);
  assert.equal(run('host-cleanup.sh', ['--id', 'x', '--temp-prefix', 'job-'], { bin, env }).status, 0);
  assert.ok(existsSync(join(tmp, 'keep-me')) && !existsSync(join(tmp, 'job-1')));
  assert.deepEqual(calls(bin), []);
});
