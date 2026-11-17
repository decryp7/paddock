import assert from 'node:assert/strict';
import { test } from 'node:test';
import { goldenSites, isGolden } from '../src/checks/C131-golden-machine.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const ALLOW = 'conformance/c131-allow.json';
const kinds = (text) => goldenSites(text).map((s) => `${s.line}:${s.kind}`);

test('C131 bad: the ids and paths that failed on another machine (WP-2, train 14)', () => {
  // The guardian golden with the Mac's uid baked in, as JSON, as key=value and as `id` output.
  assert.deepEqual(kinds('{\n  "uid": 501,\n  "gid": 20\n}\n'), ['2:id', '3:id']);
  assert.deepEqual(kinds('uid=501(mia) gid=20(staff)\nEUID: 0\n'), ['1:id', '2:id']);
  assert.deepEqual(kinds('owner: Some(uid 1000)'), ['1:id']);
  // Home and temp paths of a person, a user folder on Windows (also escaped in JSON), a temp folder of the OS.
  assert.deepEqual(kinds('cwd=/Users/someone/work\nx=/home/runner/x\n'), ['1:home', '2:home']);
  assert.deepEqual(kinds('"C:\\\\Users\\\\Bo\\\\AppData" C:\\Users\\Bo\\x'), ['1:home']);
  assert.deepEqual(kinds('/var/folders/zz/abc/T/x\n/private/tmp/run1\n/tmp/work-1\nC:\\Users\\a\\AppData\\Local\\Temp\\x'), ['1:temp', '2:temp', '3:temp', '4:home', '4:temp']);
  // Astral characters are one character, and ſ folds to s in a case-insensitive match (the `u` flag; the Rust host does both).
  assert.deepEqual(kinds('uid😀😀😀😀=1\nuſer_id=5\nK uid: 3\n'), ['1:id', '2:id', '3:id']);
  // CRLF and non-ASCII text around a site.
  assert.deepEqual(kinds('é😀 uid=1\r\nclean\r\n/home/bï\r\n'), ['1:id', '3:home']);
});

test('C131 good: placeholders, near misses and other text', () => {
  assert.deepEqual(kinds('/Users/<name>/x /home/$USER /home/ /Users/ and uid=<uid>'), []);
  assert.deepEqual(kinds('a guid 501, a squid 20, the uid_max setting'), []);
  assert.deepEqual(kinds('user mia on host m9-mac-mini with 17179869184 bytes and cpus: 10'), [], 'this machine is not read');
});

test('C131 reads only golden folders and files', () => {
  for (const f of ['tools/x/tests/golden/a.json', 'golden/a.txt', 'crates/c/tests/goldens/b.plist', 'a/b.golden', 'a/b.snap']) assert.equal(isGolden(f), true, f);
  for (const f of ['tools/x/tests/golden/a.png', 'src/golden.rs', 'docs/goldenrod/a.md', 'conformance/c131-allow.json']) assert.equal(isGolden(f), false, f);
});

test('C131 through the runner: the planted uid is found, warnings only, allow entries by count, a broken allow file is one finding', async () => {
  const root = makeApp({
    [ALLOW]: JSON.stringify({ allow: [{ path: 'x/tests/golden/b.json', site: '/Users/ana', reason: 'an invented user the test passes in', count: 2 }] }),
    'x/tests/golden/a.json': '{"uid": 501}\n',
    'x/tests/golden/b.json': '"/Users/ana/a"\n"/Users/ana/b"\n"/Users/ana/c"\n',
    'x/tests/golden/c.json': '{"uid": "<uid>"}\n',
    'x/tests/fixtures/d.json': '{"uid": 501}\n',
    'x/tests/golden/e.png': 'uid 501',
    'x/tests/f.snap': 'uid: 7\n',
  });
  const found = await findingsOf('C131', root);
  assert.deepEqual(found.map((f) => [f.file, f.line, f.severity]).sort(), [['x/tests/f.snap', 1, 'warn'], ['x/tests/golden/a.json', 1, 'warn'], ['x/tests/golden/b.json', 3, 'warn']]);
  assert.match(found.find((f) => f.file.endsWith('b.json')).message, /beyond the count/);
  for (const text of [null, 'nope', '{}']) {
    const broken = await findingsOf('C131', makeApp({ 'x/tests/golden/a.json': '{"uid": 501}\n', ...(text === null ? {} : { [ALLOW]: text }) }));
    assert.deepEqual(broken.map((f) => [f.file, f.outcome]), [[ALLOW, 'check-broken']]);
  }
  const stale = await findingsOf('C131', makeApp({ [ALLOW]: JSON.stringify({ allow: [{ path: 'x/tests/golden/a.json', site: 'gone', reason: 'a stale entry here' }] }), 'x/tests/golden/a.json': 'fine\n' }));
  assert.match(stale.map((f) => f.message).join('\n'), /stale allow entry/);
});

test('C131 gives the same findings whoever runs it: the environment is not read', async () => {
  const root = makeApp({ [ALLOW]: '{"allow": []}', 'x/tests/golden/a.json': `{"user": "${process.env.USER ?? 'nobody'}", "uid": 9}\n` });
  const before = (await findingsOf('C131', root)).map((f) => [f.file, f.line, f.message]);
  const saved = { ...process.env };
  try {
    Object.assign(process.env, { USER: 'someoneelse', HOME: '/h/x', LOGNAME: 'someoneelse', HOSTNAME: 'otherhost' });
    assert.deepEqual((await findingsOf('C131', root)).map((f) => [f.file, f.line, f.message]), before);
  } finally {
    Object.assign(process.env, saved);
  }
});
