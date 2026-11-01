import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { enclosingTest, ENDPOINT_PATTERN, findAllCreators, findCreators, groupLeaks, inTest, isTestPath, leakPattern, LIMITS, listEndpointLeaks, listLeaks, main, newLeaks, sharedDirs, sourceFiles, verdict } from './check-temp-leaks.mjs';

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

// Rule 56: a leak names the test that left it, not only the folder (a triager had to grep the tree by hand, 2026-10-31).
// The fixtures copy the real shapes: debrief-* and imt-* scratch folders built with format!, a #[cfg(test)] module, a
// production line that spells the same stem, and debrief-store-* made by two tests.
function sources(dir) {
  mkdirSync(join(dir, 'src', 'deep'), { recursive: true });
  mkdirSync(join(dir, 'tests'), { recursive: true });
  mkdirSync(join(dir, 'target', 'debug'), { recursive: true });
  writeFileSync(
    join(dir, 'src', 'perf.rs'),
    [
      'pub fn spool_dir() -> PathBuf {',
      '    std::env::temp_dir().join(format!("imt-{}", std::process::id()))',
      '}',
      '#[test]',
      'fn writes_the_preset() {',
      '    let dir = std::env::temp_dir().join(format!("debrief-tools-perfmon-{}", std::process::id()));',
      '}',
      '#[test]',
      'fn overwrite_needs_a_yes() {',
      '    let dir = std::env::temp_dir().join(format!("debrief-tools-perfmon-ow-{}", std::process::id()));',
      '}',
      '#[cfg(test)]',
      'mod tests {',
      '    fn scratch() -> PathBuf {',
      '        std::env::temp_dir().join(format!("imt-{}", COUNTER.fetch_add(1, SeqCst)))',
      '    }',
      '}',
    ].join('\n'),
  );
  writeFileSync(join(dir, 'src', 'deep', 'store.spec.ts'), "describe('x', () => {\n  it('keeps a scratch store', () => {\n    const d = `debrief-store-${n}`;\n  });\n});\n");
  writeFileSync(join(dir, 'tests', 'stores.rs'), 'fn opens_two_stores() {\n    let d = format!("debrief-store-{}", id);\n}\n');
  writeFileSync(join(dir, 'src', 'name.rs'), 'pub fn folder(kind: &str) -> String {\n    format!("debrief-{kind}-{}", std::process::id())\n}\n');
  writeFileSync(join(dir, 'target', 'debug', 'built.rs'), 'format!("debrief-tools-perfmon-{}")');
}

test('test code is recognised: test paths, #[cfg(test)] modules, #[test] fns and it()/test() blocks', () => {
  assert.ok(isTestPath('crates/x/tests/it/store.rs') && isTestPath('app/src/a.spec.ts') && isTestPath('scripts/b.test.mjs'));
  assert.ok(!isTestPath('crates/x/src/store.rs'));
  const rust = ['fn prod() {', '  a', '}', '#[tokio::test]', 'async fn t() {', '  b', '}', '#[cfg(test)]', 'mod tests {', '  c', '}'];
  assert.equal(inTest('src/x.rs', rust, 1), false);
  assert.equal(inTest('src/x.rs', rust, 5), true, '#[tokio::test] above the fn');
  assert.equal(inTest('src/x.rs', rust, 9), true, 'after #[cfg(test)]');
  assert.equal(inTest('src/x.ts', ["it('does', () => {", '  x'], 1), true);
  assert.equal(enclosingTest(['fn a() {', '  x', '}', 'fn b() {', '  y'], 4), 'b');
  assert.equal(enclosingTest(["test('does a thing', () => {", '  y'], 1), 'does a thing');
  assert.equal(enclosingTest(['let x = 1;'], 0), '');
});

test('imt-*: the one strict hit inside a test is the culprit, ahead of the production line that spells the same stem', () => {
  const s = scratch();
  try {
    sources(s.dir);
    const hits = findAllCreators(['imt-*'], sourceFiles(s.dir), { root: s.dir }).get('imt-*');
    assert.deepEqual(hits.map((h) => [h.where, h.strict, h.test]), [
      [join('src', 'perf.rs') + ':15 (in scratch)', true, true],
      [join('src', 'perf.rs') + ':2 (in spool_dir)', true, false],
    ]);
    assert.equal(verdict('imt-*', hits), `imt-* from ${join('src', 'perf.rs')}:15 (in scratch)`);
  } finally {
    s.done();
  }
});

test('debrief-store-*: two tests make it, so both are named as candidates, never one culprit', () => {
  const s = scratch();
  try {
    sources(s.dir);
    const hits = findAllCreators(['debrief-store-*'], sourceFiles(s.dir), { root: s.dir }).get('debrief-store-*');
    assert.equal(hits.length, 2);
    const v = verdict('debrief-store-*', hits);
    assert.match(v, /^debrief-store-\*: candidates: /);
    assert.match(v, /store\.spec\.ts:3 \(in keeps a scratch store\)/);
    assert.match(v, /stores\.rs:2 \(in opens_two_stores\)/);
  } finally {
    s.done();
  }
});

test('debrief-*: a name built at run time is only "possibly", and a longer name is never a strict hit', () => {
  const s = scratch();
  try {
    sources(s.dir);
    const files = sourceFiles(s.dir);
    const found = findAllCreators(['debrief-*', 'debrief-tools-perfmon-*'], files, { root: s.dir });
    const generic = found.get('debrief-*');
    assert.ok(generic.length > 0 && generic.every((h) => !h.strict), 'every debrief-* line is followed by a letter');
    assert.match(verdict('debrief-*', generic), /^debrief-\*: possibly \(name built at run time\): /);
    const perf = found.get('debrief-tools-perfmon-*');
    assert.deepEqual(perf.map((h) => h.strict), [true, false], 'the -ow- line ranks last, as a loose hit');
    assert.equal(verdict('debrief-tools-perfmon-*', perf), `debrief-tools-perfmon-* from ${join('src', 'perf.rs')}:6 (in writes_the_preset)`);
    assert.equal(verdict('debrief-made-at-runtime-*', []), '');
    assert.ok(!files.some((f) => f.includes('target')), 'target/ is never searched');
  } finally {
    s.done();
  }
});

test('the search reads each file once for all patterns, and a pattern found under another folder searches its basename', () => {
  const s = scratch();
  try {
    sources(s.dir);
    const files = sourceFiles(s.dir);
    const reads = new Map();
    const read = (f) => {
      reads.set(f, (reads.get(f) ?? 0) + 1);
      return readFileSync(f, 'utf8');
    };
    const found = findAllCreators(['imt-*', 'debrief-store-*', '/tmp/debrief-tools-perfmon-*'], files, { root: s.dir, read });
    assert.equal(reads.size, files.length);
    assert.ok([...reads.values()].every((n) => n === 1));
    assert.equal(found.get('/tmp/debrief-tools-perfmon-*')[0].where, join('src', 'perf.rs') + ':6 (in writes_the_preset)');
  } finally {
    s.done();
  }
});

test('the limits hold: maxFiles bounds the listing, maxBytes skips a large file, maxHits bounds the places', () => {
  const s = scratch();
  try {
    sources(s.dir);
    assert.equal(sourceFiles(s.dir, { maxFiles: 2 }).length, 2);
    assert.ok(LIMITS.maxFiles >= 1000 && LIMITS.maxBytes >= 1024 && LIMITS.maxHits >= 1);
    const files = sourceFiles(s.dir);
    assert.deepEqual(findCreators('imt-*', files, s.dir, { maxBytes: 10 }), [], 'every fixture file is over 10 bytes');
    assert.equal(findCreators('imt-*', files, s.dir, { maxHits: 1 }).length, 1);
  } finally {
    s.done();
  }
});

test('a leak ends with one ::error:: naming the culprit test, paths relative to each source root, and each folder\'s time', () => {
  const tmp = scratch();
  const src = scratch();
  const other = scratch();
  try {
    sources(src.dir);
    mkdirSync(join(other.dir, 'tests'));
    writeFileSync(join(other.dir, 'tests', 'more.rs'), 'fn makes_a_cache() {\n    let d = format!("debrief-cache-{}", id);\n}\n');
    mkdirSync(join(tmp.dir, 'debrief-tools-perfmon-4242'));
    mkdirSync(join(tmp.dir, 'debrief-cache-7'));
    const a = quiet();
    assert.equal(main(['--dir', tmp.dir, '--source', src.dir, '--source', other.dir], a.log), 1);
    const text = a.out.join('\n');
    assert.match(text, /debrief-tools-perfmon-\*\n\s+src.perf\.rs:6 \(in writes_the_preset\)\n\s+src.perf\.rs:10 .*\(possibly: name built at run time\)/);
    assert.match(text, /debrief-cache-\*\n\s+tests.more\.rs:2 \(in makes_a_cache\)\n/, 'relative to its own root');
    assert.match(text, /debrief-tools-perfmon-4242 {2}\d{4}-\d\d-\d\dT/);
    const errors = a.out.filter((l) => l.startsWith('::error::'));
    assert.equal(errors.length, 1);
    assert.match(errors[0], /2 temp folder\(s\) left behind by the tests: .*Next step: .*debrief-tools-perfmon-\* from src.perf\.rs:6 \(in writes_the_preset\)/);
    assert.match(errors[0], /debrief-cache-\* from tests.more\.rs:2 \(in makes_a_cache\)/);
  } finally {
    tmp.done();
    src.done();
    other.done();
  }
});

test('many loose lines never crowd out the one strict line in a test: strict and loose hits are capped apart', () => {
  const s = scratch();
  try {
    mkdirSync(join(s.dir, 'src'));
    const loose = Array.from({ length: 30 }, (_, i) => `// debrief-imports-log mentioned ${i}`);
    writeFileSync(join(s.dir, 'src', 'a.rs'), [...loose, '#[test]', 'fn writes_imports() {', '    let d = format!("debrief-imports-{}", id);', '}'].join('\n'));
    const hits = findAllCreators(['debrief-imports-*'], sourceFiles(s.dir), { root: s.dir, maxHits: 1 }).get('debrief-imports-*');
    assert.deepEqual(hits.map((h) => [h.where, h.strict, h.test]), [[join('src', 'a.rs') + ':33 (in writes_imports)', true, true]]);
  } finally {
    s.done();
  }
});
