import assert from 'node:assert/strict';
import { test } from 'node:test';
import { IMPLEMENTATION, parseAllow, readable, sites } from '../src/checks/C130-stand-in-fixtures.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const ALLOW = JSON.stringify({ allow: [{ path: 'tools/t/tests/it/ok.rs', site: '#!/bin/sh', reason: 'baseline: planted before StandIn', count: 2 }] });
const lines = (found) => found.map(({ line }) => line);

test('C130 sites: shebang text in a test file, or after the first cfg(test) line of a source file', () => {
  const planted = 'std::fs::write(&p, "#!/bin/sh\\nexit 3\\n").unwrap();\n';
  assert.deepEqual(lines(sites('tools/t/tests/it/a.rs', `fn a() {}\n${planted}`)), [2]);
  assert.deepEqual(lines(sites('crates/x/src/tests.rs', planted)), [1]);
  assert.deepEqual(lines(sites('crates/x/src/a_test.rs', planted)), [1]);
  // A source file: only from its first cfg(test) line on; production shell text above it is not a fixture.
  assert.deepEqual(sites('crates/x/src/a.rs', `const SHIM: &str = "#!/bin/sh\\n";\n${planted}`), []);
  assert.deepEqual(lines(sites('crates/x/src/a.rs', `const SHIM: &str = "#!/bin/sh\\n";\n#[cfg(test)]\nmod tests {\n${planted}}\n`)), [4]);
  for (const cfg of ['#[cfg(all(test, unix))]', '#[cfg(all(unix, test))]', '#[cfg(any(test, windows))]']) {
    assert.deepEqual(lines(sites('crates/x/src/a.rs', `${cfg}\nmod t {\n${planted}}\n`)), [3], cfg);
  }
});

test('C130 sites: a file that calls write_program( is read whole, test-support files included', () => {
  const body = 'fn install(p: &Path) {\n    write_program(p, "#!/bin/sh\\nexit 0\\n").unwrap();\n}\n';
  assert.deepEqual(lines(sites('crates/x/src/fake.rs', body)), [2]);
  // Without the call, production shell text in a source file is still not a fixture.
  assert.deepEqual(sites('crates/x/src/fake.rs', 'const SHIM: &str = "#!/bin/sh\\n";\n'), []);
});

test('C130 sites: sh, bash and zsh, plain and through env; other text is not a shebang', () => {
  for (const sb of ['#!/bin/sh', '#!/bin/bash', '#!/usr/bin/env bash', '#!/usr/bin/env sh', '#!/bin/zsh', '#!/usr/bin/env  bash']) {
    assert.equal(sites('x/tests/a.rs', `let s = "${sb}\\n";\n`).length, 1, sb);
  }
  for (const other of ['#!/usr/bin/env node', '#!/usr/bin/python3', '#!/bin/shell-like', 'let s = "sh -c x";']) {
    assert.deepEqual(sites('x/tests/a.rs', `${other}\n`), [], other);
  }
});

test('C130 good: comments, fixtures, vendored code and the check\'s own files are not read', () => {
  assert.deepEqual(sites('x/tests/a.rs', '// writes #!/bin/sh in the old test\n/// doc #!/bin/sh\n'), []);
  for (const p of ['x/tests/fixtures/a.rs', 'target/x/tests/a.rs', 'vendor/y/tests/a.rs', 'x/tests/a.sh', ...IMPLEMENTATION]) assert.equal(readable(p), false, p);
});

test('C130 waivers: one with a reason on the line or the line above is `waived` (and counted by the allow list), a bare or distant one is not', () => {
  const w = (text) => sites('x/tests/a.rs', text).map(({ line, waived }) => [line, waived]);
  assert.deepEqual(w('let s = "#!/bin/sh\\n"; // conformance: stand-in — the hook under test is a script\n'), [[1, true]]);
  assert.deepEqual(w('// conformance: stand-in — a script by design\nlet s = "#!/bin/sh\\n";\n'), [[2, true]]);
  assert.deepEqual(w('// conformance: stand-in — a script by design\n\nlet s = "#!/bin/sh\\n";\n'), [[3, false]], 'the line or the one above only');
  assert.deepEqual(w('let s = "#!/bin/sh\\n"; // conformance: stand-in\n'), [[1, false]]);
  assert.deepEqual(w('let s = "#!/bin/sh\\n"; // conformance: stand-in - ab\n'), [[1, false]], 'a reason is at least three characters');
  assert.deepEqual(w('let s = "#!/bin/sh\\n"; // conformance: stand-in -- a\\u{1}b\n'), [[1, true]]);
  // CRLF: the carriage return is not part of the reason; an emoji is two UTF-16 units, so one emoji and a letter is a reason, an emoji alone is not.
  assert.deepEqual(w('let s = "#!/bin/sh\\n"; // conformance: stand-in — ab\r\n'), [[1, false]]);
  assert.deepEqual(w('let s = "#!/bin/sh\\n"; // conformance: stand-in — abc\r\n'), [[1, true]]);
  assert.deepEqual(w('let s = "#!/bin/sh\\n"; // conformance: stand-in — \u{1F600}a\n'), [[1, true]]);
  assert.deepEqual(w('let s = "#!/bin/sh\\n"; // conformance: stand-in — \u{1F600}\n'), [[1, false]]);
});

test('C130 allow list: a path, the shebang text as the site, a reason and a count', () => {
  assert.deepEqual(parseAllow(ALLOW), [{ path: 'tools/t/tests/it/ok.rs', site: '#!/bin/sh', reason: 'baseline: planted before StandIn', count: 2 }]);
  assert.throws(() => parseAllow('{}'), /no `allow` array/);
  assert.throws(() => parseAllow('{"allow":[{"path":"x.rs","site":"#!/bin/sh","reason":"short"}]}'), /reason of at least 8/);
});

test('C130 through the runner: warnings only; the allow list spares its sites, one beyond its count and a stale entry are named', async () => {
  const sh = 'std::fs::write(&p, "#!/bin/sh\\nexit 3\\n").unwrap();\n';
  const root = makeApp({
    'conformance/c130-allow.json': ALLOW,
    'tools/t/tests/it/ok.rs': `${sh}${sh}${sh}`,
    'tools/t/tests/it/bad.rs': `fn b() {}\n${sh}`,
    'tools/t/src/lib.rs': `const SHIM: &str = "#!/bin/sh\\n";\n`,
  });
  const found = await findingsOf('C130', root);
  assert.deepEqual(found.map((f) => [f.file, f.line, f.severity]), [['tools/t/tests/it/bad.rs', 2, 'warn'], ['tools/t/tests/it/ok.rs', 3, 'warn']]);
  assert.match(found[1].message, /beyond the count/);
  const stale = makeApp({ 'conformance/c130-allow.json': ALLOW, 'tools/t/tests/it/ok.rs': sh });
  const s = await findingsOf('C130', stale);
  assert.equal(s.length, 1);
  assert.match(s[0].message, /stale|matched/i);
  const none = makeApp({ 'tools/t/tests/it/a.rs': sh });
  const broken = await findingsOf('C130', none);
  assert.equal(broken.length, 1);
  assert.equal(broken[0].file, 'conformance/c130-allow.json');
});
