// A port is proven equal to the script it replaces on every OS the step runs on (docs/plans/ci-in-rust.md section 7, ER-9). Three
// ports in a row had a parity file that is Unix-only, so the Windows leg proved nothing (ER-21). Every integration test file of
// paddock-dev that is Unix-only as a whole (`#![cfg(unix)]`) says what covers Windows: `// windows-twin: <file>` (a file of the
// folder that has no file-wide `cfg(unix)`) or `// windows-twin: none: <reason>`.
import assert from 'node:assert/strict';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { PADDOCK } from './helpers.mjs';

const DIR = join(PADDOCK, 'tools', 'paddock-dev', 'tests', 'it');
const unixOnly = (text) => /^#!\[cfg\(unix\)\]\s*$/m.test(text);

/** What is wrong with the Unix-only files of `files` ({ name: text }). */
export function twinProblems(files) {
  const found = [];
  for (const [name, text] of Object.entries(files)) {
    if (!unixOnly(text)) continue;
    const m = /^\/\/ windows-twin: (.+)$/m.exec(text);
    if (!m) {
      found.push(`${name}: Unix-only with no "// windows-twin:" line`);
    } else if (m[1].startsWith('none:')) {
      if (m[1].slice(5).trim().length < 10) found.push(`${name}: "none" needs a reason`);
    } else {
      const twin = files[m[1].trim()];
      if (twin === undefined) found.push(`${name}: the twin ${m[1].trim()} does not exist`);
      else if (unixOnly(twin)) found.push(`${name}: the twin ${m[1].trim()} is Unix-only too`);
    }
  }
  return found;
}

const read = () => Object.fromEntries(readdirSync(DIR).filter((f) => f.endsWith('.rs')).map((f) => [f, readFileSync(join(DIR, f), 'utf8')]));

test('every Unix-only integration test file of paddock-dev names the file that covers Windows', () => {
  const files = read();
  assert.ok(existsSync(DIR) && Object.values(files).filter(unixOnly).length >= 5, 'the Unix-only files are found');
  assert.deepEqual(twinProblems(files), []);
});

test('the check sees a Unix-only parity file without a twin, a missing twin, a Unix-only twin and a bare "none"', () => {
  const unix = '//! x\n#![cfg(unix)]\n';
  assert.equal(twinProblems({ 'a.rs': unix }).length, 1);
  assert.equal(twinProblems({ 'a.rs': '// windows-twin: b.rs\n' + unix }).length, 1, 'missing twin');
  assert.equal(twinProblems({ 'a.rs': '// windows-twin: b.rs\n' + unix, 'b.rs': '// windows-twin: none: a stand-in with a reason\n' + unix }).length, 1, 'Unix-only twin');
  assert.equal(twinProblems({ 'a.rs': '// windows-twin: none:\n' + unix }).length, 1);
  assert.deepEqual(twinProblems({ 'a.rs': '// windows-twin: b.rs\n' + unix, 'b.rs': '//! y\n' }), []);
  assert.deepEqual(twinProblems({ 'a.rs': '// windows-twin: none: runs a shell script that Windows jobs run under Git bash\n' + unix }), []);
});
