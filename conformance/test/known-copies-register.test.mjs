import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const register = readFileSync(fileURLToPath(new URL('../../docs/known-copies.toml', import.meta.url)), 'utf8');

/** The `id` of every `[[mechanism]]` table, in file order (a copy table has no `id`). */
export const mechanismIds = (text) => {
  const ids = [];
  let inMechanism = false;
  for (const line of text.split(/\r?\n/)) {
    if (/^\[\[mechanism\]\]\s*$/.test(line)) inMechanism = true;
    else if (/^\s*\[\[/.test(line)) inMechanism = false;
    else if (inMechanism) {
      const m = /^id\s*=\s*"([^"]+)"/.exec(line);
      if (m) {
        ids.push(m[1]);
        inMechanism = false;
      }
    }
  }
  return ids;
};

test('the id reader finds each mechanism id and skips copy tables', () => {
  const text = '[[mechanism]]\nid = "a"\n\n  [[mechanism.copy]]\n  paths = ["x"]\n\n[[mechanism]]\nid = "b"\n';
  assert.deepEqual(mechanismIds(text), ['a', 'b']);
});

test('the known-copies register has no duplicate mechanism id', () => {
  const ids = mechanismIds(register);
  assert.ok(ids.length > 0, 'the register lists mechanisms');
  const dupes = [...new Set(ids.filter((id, i) => ids.indexOf(id) !== i))];
  assert.deepEqual(dupes, [], `docs/known-copies.toml repeats mechanism ids (a bad merge or edit duplicated a block): ${dupes.join(', ')}`);
});
