import assert from 'node:assert/strict';
import { test } from 'node:test';
import { goldenSites, isGolden, machineFacts, machinePatterns } from '../src/checks/C130-golden-machine.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const ALLOW = 'conformance/c130-allow.json';
const FACTS = { user: 'mia', home: '/Users/mia', host: 'm9-mac-mini.local', memBytes: 17179869184, cpus: 10 };
const kinds = (text, facts = FACTS) => goldenSites(text, facts).map((s) => `${s.line}:${s.kind}`);

test('C130 bad: the ids, paths and machine values that failed on another machine (WP-2, train 14)', () => {
  // The guardian golden with the Mac's uid baked in, as JSON, as key=value and as `id` output.
  assert.deepEqual(kinds('{\n  "uid": 501,\n  "gid": 20\n}\n'), ['2:id', '3:id']);
  assert.deepEqual(kinds('uid=501(mia) gid=20(staff)\nEUID: 0\n'), ['1:id', '1:user', '2:id']);
  assert.deepEqual(kinds('owner: Some(uid 1000)'), ['1:id']);
  // Home and temp paths of a person, a user folder on Windows (also escaped in JSON), a temp folder of the OS.
  assert.deepEqual(kinds('cwd=/Users/someone/work\nx=/home/runner/x\n'), ['1:home', '2:home']);
  assert.deepEqual(kinds('"C:\\\\Users\\\\Bo\\\\AppData" C:\\Users\\Bo\\x'), ['1:home']);
  assert.deepEqual(kinds('/var/folders/zz/abc/T/x\n/private/tmp/run1\n/tmp/work-1\nC:\\Users\\a\\AppData\\Local\\Temp\\x'), ['1:temp', '2:temp', '3:temp', '4:home', '4:temp']);
  // This machine: its user, host name, home folder, RAM and CPU count.
  assert.deepEqual(kinds('built by mia\n'), ['1:user']);
  assert.deepEqual(kinds('host m9-mac-mini\nhost m9-mac-mini.local\n'), ['1:host', '2:host']);
  assert.deepEqual(kinds('ram 17179869184\nram_kb 16777216\nram_mb 16384\n'), ['1:ram', '2:ram', '3:ram']);
  assert.deepEqual(kinds('"cpus": 10\ncores = 10\n'), ['1:cpus', '2:cpus']);
  assert.deepEqual(kinds('home=/Users/mia/x'), ['1:home', '1:user', '1:home-of-this-machine']);
});

test('C130 good: placeholders, near misses, small numbers and facts too common to tell', () => {
  assert.deepEqual(kinds('/Users/<name>/x /home/$USER /home/ /Users/ and uid=<uid> gid placeholder'), []);
  assert.deepEqual(kinds('a guid 501, a squid 20, the uid_max setting'), []);
  assert.deepEqual(kinds('mia2 miami amia'), [], 'a word, not a part of one');
  assert.deepEqual(kinds('ram 171798691841 and 1717986918'), [], 'a longer or shorter number is another number');
  assert.deepEqual(kinds('cpus 100 and cpus: 1.10'), []);
  assert.deepEqual(machinePatterns({ user: 'root', home: '/root', host: 'localhost', memBytes: 1, cpus: 1 }), [], 'root, localhost and a one-core machine tell nothing');
  assert.deepEqual(kinds('user root /tmp/ x', { user: 'root', home: '/root' }), []);
});

test('C130 reads the machine from the environment, or from PADDOCK_GOLDEN_FACTS', () => {
  assert.deepEqual(machineFacts({ PADDOCK_GOLDEN_FACTS: JSON.stringify(FACTS) }), FACTS);
  const own = machineFacts({ USER: 'zed', HOME: '/h/zed' });
  assert.equal(own.user, 'zed');
  assert.equal(own.home, '/h/zed');
  assert.ok(own.memBytes > 0 && own.cpus >= 1);
});

test('C130 reads only golden folders and files', () => {
  for (const f of ['tools/x/tests/golden/a.json', 'golden/a.txt', 'crates/c/tests/goldens/b.plist', 'a/b.golden']) assert.equal(isGolden(f), true, f);
  for (const f of ['tools/x/tests/golden/a.png', 'src/golden.rs', 'docs/goldenrod/a.md', 'conformance/c130-allow.json']) assert.equal(isGolden(f), false, f);
});

test('C130 through the runner: the planted uid is found, warnings only, allow entries by count, a broken allow file is one finding', async () => {
  process.env.PADDOCK_GOLDEN_FACTS = JSON.stringify(FACTS);
  try {
    const root = makeApp({
      [ALLOW]: JSON.stringify({ allow: [{ path: 'x/tests/golden/b.json', site: '/Users/ana', reason: 'an invented user the test passes in', count: 2 }] }),
      'x/tests/golden/a.json': '{"uid": 501}\n',
      'x/tests/golden/b.json': '"/Users/ana/a"\n"/Users/ana/b"\n"/Users/ana/c"\n',
      'x/tests/golden/c.json': '{"uid": "<uid>"}\n',
      'x/tests/fixtures/d.json': '{"uid": 501}\n',
      'x/tests/golden/e.png': 'uid 501',
    });
    const found = await findingsOf('C130', root);
    assert.deepEqual(found.map((f) => [f.file, f.line, f.severity]).sort(), [['x/tests/golden/a.json', 1, 'warn'], ['x/tests/golden/b.json', 3, 'warn']]);
    assert.match(found.find((f) => f.file.endsWith('b.json')).message, /beyond the count/);
    assert.match(found[0].fix ?? found[0].message, /norm\(\)/);
    for (const text of [null, 'nope', '{}']) {
      const broken = await findingsOf('C130', makeApp({ 'x/tests/golden/a.json': '{"uid": 501}\n', ...(text === null ? {} : { [ALLOW]: text }) }));
      assert.deepEqual(broken.map((f) => [f.file, f.outcome]), [[ALLOW, 'check-broken']]);
    }
    const stale = await findingsOf('C130', makeApp({ [ALLOW]: JSON.stringify({ allow: [{ path: 'x/tests/golden/a.json', site: 'gone', reason: 'a stale entry here' }] }), 'x/tests/golden/a.json': 'fine\n' }));
    assert.match(stale.map((f) => f.message).join('\n'), /stale allow entry/);
  } finally {
    delete process.env.PADDOCK_GOLDEN_FACTS;
  }
});
