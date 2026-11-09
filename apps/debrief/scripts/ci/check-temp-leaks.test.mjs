import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { ENDPOINT_PATTERN, groupLeaks, leakPattern, listEndpointLeaks, listLeaks, main, newLeaks, sharedDirs } from './check-temp-leaks.mjs';

const quiet = () => {
  const out = [];
  return { out, log: { log: (m) => out.push(m), error: (m) => out.push(m) } };
};

function scratch() {
  const dir = mkdtempSync(join(tmpdir(), 'leakcheck-'));
  return { dir, done: () => rmSync(dir, { recursive: true, force: true }) };
}

test('lists only debrief-* and imt-* entries, sorted', () => {
  const s = scratch();
  try {
    for (const n of ['debrief-b', 'imt-a', 'other', 'debriefx', 'xdebrief-1']) mkdirSync(join(s.dir, n));
    assert.deepEqual(listLeaks(s.dir), ['debrief-b', 'imt-a']);
    assert.deepEqual(listLeaks(join(s.dir, 'missing')), []);
  } finally {
    s.done();
  }
});

test('newLeaks ignores what the baseline already had', () => {
  assert.deepEqual(newLeaks(['debrief-a', 'debrief-b'], ['debrief-a']), ['debrief-b']);
  assert.deepEqual(newLeaks(['debrief-a']), ['debrief-a']);
});

test('snapshot then check passes when nothing new appeared and fails naming the new folder', () => {
  const s = scratch();
  try {
    mkdirSync(join(s.dir, 'debrief-old'));
    const file = join(s.dir, 'base.json');
    const a = quiet();
    assert.equal(main(['--dir', s.dir, '--snapshot', file], a.log), 0);
    assert.deepEqual(JSON.parse(readFileSync(file, 'utf8')), ['debrief-old']);
    assert.equal(main(['--dir', s.dir, '--check', file], quiet().log), 0);
    mkdirSync(join(s.dir, 'debrief-find-1'));
    const b = quiet();
    assert.equal(main(['--dir', s.dir, '--check', file], b.log), 1);
    assert.match(b.out.join('\n'), /debrief-find-1/);
    assert.doesNotMatch(b.out.join('\n'), /debrief-old/);
  } finally {
    s.done();
  }
});

test('without a baseline every matching folder counts; a corrupt baseline counts as empty', () => {
  const s = scratch();
  try {
    mkdirSync(join(s.dir, 'imt-x'));
    assert.equal(main(['--dir', s.dir], quiet().log), 1);
    const bad = join(s.dir, 'bad.json');
    writeFileSync(bad, 'not json');
    assert.equal(main(['--dir', s.dir, '--check', bad], quiet().log), 1);
  } finally {
    s.done();
  }
});

test('a leaked name is reduced to its fixture: pids, counters, hex ids and tempfile suffixes go', () => {
  assert.equal(leakPattern('debrief-ai-find-36441-0'), 'debrief-ai-find-*');
  assert.equal(leakPattern('debrief-plugin-0306efa26cfd'), 'debrief-plugin-*');
  assert.equal(leakPattern('debrief-snap-tiny-512-3'), 'debrief-snap-tiny-*');
  assert.equal(leakPattern('debrief-core-names-search-Ab3xYz'), 'debrief-core-names-search-*');
  assert.equal(leakPattern('debrief-app-test-global-log-38257'), 'debrief-app-test-global-log-*');
  assert.equal(leakPattern('debrief-old'), 'debrief-old', 'nothing per-run: the name itself');
  assert.equal(leakPattern('debrief-123'), 'debrief-*');
});

test('the offenders are listed with their counts, most first', () => {
  const names = ['debrief-plugin-aa11', 'debrief-ai-find-1-0', 'debrief-plugin-bb22', 'debrief-ai-find-2-0', 'debrief-plugin-cc33', 'imt-x'];
  assert.deepEqual(groupLeaks(names), [
    { pattern: 'debrief-plugin-*', count: 3 },
    { pattern: 'debrief-ai-find-*', count: 2 },
    { pattern: 'imt-x', count: 1 },
  ]);
  const s = scratch();
  try {
    for (const n of names) mkdirSync(join(s.dir, n));
    const out = quiet();
    assert.equal(main(['--dir', s.dir], out.log), 1);
    const text = out.out.join('\n');
    assert.match(text, /Offenders[\s\S]*3 {2}debrief-plugin-\*[\s\S]*2 {2}debrief-ai-find-\*/);
    assert.match(text, /debrief-plugin-aa11/, 'the folders are listed too');
  } finally {
    s.done();
  }
});

test('the shared folders are scanned for endpoint-shaped names only, never for other files', () => {
  assert.ok(ENDPOINT_PATTERN.test('debrief-plugin-372eb9bd2f3d'));
  assert.ok(ENDPOINT_PATTERN.test('debrief-ipc-0123456789ab'));
  // A test's own prefix with a random part (debrief-ipc tests/it/launch.rs: `debrief-test-gone-<8 hex>`), plus the 12 hex.
  assert.ok(ENDPOINT_PATTERN.test('debrief-test-gone-1a2b3c4d-0123456789ab'));
  assert.ok(ENDPOINT_PATTERN.test('dbwAb3dEf'));
  for (const other of ['debrief-plugin-372eb9bd2f3', 'debrief-plugin-372eb9bd2f3dd', 'debrief-parity-x-1-ab', 'debrief', 'systemd-private-0123456789ab', 'dbw', 'dbwAb3dEf9', 'plugin-372eb9bd2f3d']) {
    assert.ok(!ENDPOINT_PATTERN.test(other), other);
  }
  const s = scratch();
  try {
    for (const n of ['debrief-plugin-372eb9bd2f3d', 'dbwAb3dEf', 'debrief-parity-x-1-ab', 'unrelated', 'user-0123456789ab']) mkdirSync(join(s.dir, n));
    assert.deepEqual(listEndpointLeaks(s.dir), [`${s.dir}/dbwAb3dEf`, `${s.dir}/debrief-plugin-372eb9bd2f3d`]);
    assert.deepEqual(listEndpointLeaks(join(s.dir, 'missing')), []);
  } finally {
    s.done();
  }
});

test('sharedDirs names /tmp and the runtime folder, once each, and not the temp folder itself', () => {
  assert.deepEqual(sharedDirs({ XDG_RUNTIME_DIR: '/run/user/1' }, '/var/tmp'), ['/tmp', '/run/user/1']);
  assert.deepEqual(sharedDirs({ XDG_RUNTIME_DIR: '/tmp' }, '/var/tmp'), ['/tmp']);
  assert.deepEqual(sharedDirs({}, '/tmp'), []);
});

test('--dir with --shared scans the given folder and the shared socket folders (a CI job\'s own TMPDIR, 2026-10-29)', () => {
  const temp = scratch();
  const shared = scratch();
  try {
    const file = join(temp.dir, '..', `${temp.dir.split('/').pop()}-base.json`);
    const args = ['--dir', temp.dir, '--shared'];
    assert.equal(main([...args, '--snapshot', file], quiet().log, () => [shared.dir]), 0);
    // A socket folder that fell back to a shared folder (the job's temp path was too long) still counts.
    mkdirSync(join(shared.dir, 'debrief-plugin-372eb9bd2f3d'));
    const out = quiet();
    assert.equal(main([...args, '--check', file], out.log, () => [shared.dir]), 1);
    rmSync(file, { force: true });
    // Without --shared, --dir alone scans only that folder, as before.
    assert.equal(main(['--dir', temp.dir], quiet().log, () => [shared.dir]), 0);
  } finally {
    temp.done();
    shared.done();
  }
});

test('--socket-dir scans the job\'s own socket folder for endpoint-shaped names, so a host job never reads the shared /tmp', () => {
  const temp = scratch();
  const sockets = scratch();
  const shared = scratch();
  try {
    const file = join(temp.dir, '..', `${temp.dir.split('/').pop()}-sock.json`);
    const args = ['--dir', temp.dir, '--socket-dir', sockets.dir];
    const never = () => [shared.dir];
    assert.equal(main([...args, '--snapshot', file], quiet().log, never), 0);
    // Someone else's socket folder in the shared folder (a local run on the same host) is not this job's leak.
    mkdirSync(join(shared.dir, 'debrief-ipc-0123456789ab'));
    assert.equal(main([...args, '--check', file], quiet().log, never), 0);
    // The job's own socket folder is checked.
    mkdirSync(join(sockets.dir, 'dbwAb3dEf'));
    assert.equal(main([...args, '--check', file], quiet().log, never), 1);
    rmSync(file, { force: true });
  } finally {
    temp.done();
    sockets.done();
    shared.done();
  }
});

test('an empty --socket-dir fails with 2 on Unix (a missing socket base is never skipped), and is allowed on Windows', () => {
  const temp = scratch();
  try {
    const out = quiet();
    assert.equal(main(['--dir', temp.dir, '--socket-dir', ''], out.log, () => [], 'darwin'), 2);
    assert.equal(main(['--dir', temp.dir, '--socket-dir', ''], quiet().log, () => [], 'linux'), 2);
    assert.equal(main(['--dir', temp.dir, '--socket-dir'], quiet().log, () => [], 'linux'), 2, 'no value at all');
    assert.equal(main(['--dir', temp.dir, '--socket-dir', ''], quiet().log, () => [], 'win32'), 0, 'named pipes: no socket folder');
  } finally {
    temp.done();
  }
});

test('a deferred-removal marker fails the check and its text (folder, error, what was left) is printed', () => {
  const s = scratch();
  try {
    writeFileSync(join(s.dir, 'debrief-leak-deferred-duck-diff-1-ab'), 'folder: /t/debrief-duck-diff-1-ab\nerror: Directory not empty\nleft (1):\nindex/seg.bin\n');
    const a = quiet();
    assert.equal(main(['--dir', s.dir], a.log), 1);
    const text = a.out.join('\n');
    assert.match(text, /Deferred removals/);
    assert.match(text, /error: Directory not empty/);
    assert.match(text, /index\/seg\.bin/);
  } finally {
    s.done();
  }
});
