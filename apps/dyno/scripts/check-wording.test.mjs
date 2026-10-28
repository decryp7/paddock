import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { ALLOW, FORBIDDEN, findOffences, visibleLines } from './check-wording.mjs';

test('Dyno promises no change to the computer anywhere people read it', () => {
  const offences = findOffences(new URL('..', import.meta.url).pathname);
  assert.deepEqual(offences, [], offences.map((o) => `${o.file}:${o.line} "${o.word}": ${o.text}`).join('\n'));
});

test('the check itself finds change vocabulary in each kind of file and honours the allow-list', () => {
  const root = mkdtempSync(join(tmpdir(), 'dyno-wording-'));
  mkdirSync(join(root, 'crates/dyno-core/src'), { recursive: true });
  mkdirSync(join(root, 'app/src/app'), { recursive: true });
  mkdirSync(join(root, 'app/src-tauri'), { recursive: true });
  writeFileSync(join(root, 'crates/dyno-core/src/a.rs'), 'const T: &str = "You can undo this";\n// undo in a comment is fine\n#[cfg(test)]\nmod t { const X: &str = "undo"; }\n');
  writeFileSync(join(root, 'app/src/app/a.ts'), "template: `<p>Quarantine it</p>`;\n// apply in a comment\n");
  writeFileSync(join(root, 'app/src-tauri/tauri.conf.json'), '{\n "shortDescription": "Tunes your PC"\n}\n');
  writeFileSync(join(root, 'Cargo.toml'), 'description = "applies fixes"\n');
  const hits = findOffences(root, []).map((o) => `${o.file}:${o.word.toLowerCase()}`).sort();
  assert.deepEqual(hits, ['Cargo.toml:applies', 'app/src-tauri/tauri.conf.json:tunes', 'app/src/app/a.ts:quarantine', 'crates/dyno-core/src/a.rs:undo']);
  const allowed = findOffences(root, [{ file: 'a.ts', text: 'Quarantine it', reason: 'test' }]).map((o) => o.file);
  assert.ok(!allowed.includes('app/src/app/a.ts'));
});

test('every allow-list entry names a reason, and the vocabulary covers the words the review named', () => {
  assert.ok(ALLOW.every((a) => a.reason && a.reason.length > 10));
  for (const w of ['tune', 'tunes', 'undo', 'undoable', 'apply', 'applied', 'quarantine', 'enable a rule', 'end process', 'change your PC', 'fix it']) {
    assert.ok(FORBIDDEN.some((re) => re.test(w)), w);
  }
  assert.deepEqual(visibleLines('x.md', 'undo'), []);
});
