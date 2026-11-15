// The Linux runner's slot count is declared once, in conformance/ci/runner-slots.json (ER-42): ./dev ci dispatch reads it, and
// the docs and workflow comments that state it are checked against it here.
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { PADDOCK } from './helpers.mjs';

const slots = JSON.parse(readFileSync(join(PADDOCK, 'conformance', 'ci', 'runner-slots.json'), 'utf8'));
const WORDS = ['zero', 'one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight', 'nine'];

test('the declared Linux slot count is a whole number above one', () => {
  assert.ok(Number.isInteger(slots.linux) && slots.linux >= 2, `linux = ${slots.linux}`);
});

test('docs/ci-schedules.md states the declared count, and no schedule takes more slots than there are', () => {
  const doc = readFileSync(join(PADDOCK, 'docs', 'ci-schedules.md'), 'utf8');
  const stated = /The Linux runner runs (\d+) jobs at once \((\d+) Linux slots, declared in `conformance\/ci\/runner-slots\.json`\)/.exec(doc);
  assert.ok(stated, 'docs/ci-schedules.md has the sentence naming the Linux slot count and its declaration');
  assert.equal(Number(stated[1]), slots.linux);
  assert.equal(Number(stated[2]), slots.linux);
  const rows = doc.split('\n').filter((l) => /^\| [\w-]+ \| `/.test(l));
  assert.ok(rows.length >= 5, 'the schedule table is read');
  for (const row of rows) {
    const cell = row.split('|').at(-2).trim();
    const n = /^(\d+)/.exec(cell);
    if (n) assert.ok(Number(n[1]) <= slots.linux, `${row.split('|')[1].trim()} takes ${n[1]} of ${slots.linux} slots`);
  }
});

test('every workflow or action comment that names "the N Linux slots" uses the declared count', () => {
  const files = [
    ...readdirSync(join(PADDOCK, '.gitea', 'workflows')).map((f) => join(PADDOCK, '.gitea', 'workflows', f)),
    ...readdirSync(join(PADDOCK, '.gitea', 'actions')).map((d) => join(PADDOCK, '.gitea', 'actions', d, 'action.yml')),
  ];
  const wrong = [];
  let seen = 0;
  for (const f of files) {
    let text;
    try {
      text = readFileSync(f, 'utf8');
    } catch {
      continue;
    }
    for (const m of text.matchAll(/of the (\w+) Linux (?:runner )?slots/g)) {
      seen++;
      if (m[1] !== WORDS[slots.linux] && m[1] !== String(slots.linux)) wrong.push(`${f.replace(PADDOCK, '')}: "${m[0]}"`);
    }
  }
  assert.ok(seen >= 3, `the comments are found (${seen})`);
  assert.deepEqual(wrong, [], `these name another count than conformance/ci/runner-slots.json (${slots.linux})`);
});
