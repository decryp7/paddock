// New workflow tests use lib/workflow.mjs (parsed jobs, steps, `if`, `needs`, outputs), not regexes over the YAML text (ER-21:
// three tests pinned `if:` lines, `needs:` lists and whole outputs and broke on intended edits). A test file that pins such text
// without importing the helper fails here; LEGACY lists the files that still do, so the list only shrinks.
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';

const dir = new URL('.', import.meta.url).pathname;
/** Pinned workflow text: a regex literal or a template that spells a `needs:` or `if:` line, or `train-skip=`-style whole outputs. */
const PINS = /\/[^/\n]*(?:needs: \\?\[|\bif: \\?\$|\\n\s+if:)[^/\n]*\/|\bincludes\(`\\n\s+if:|lines\.includes\('[\w-]+=/;
const LEGACY = new Set(['ci-actions.test.mjs', 'kept-clone.test.mjs']);

test('workflow tests read workflows through lib/workflow.mjs, not through their text', () => {
  const offenders = [];
  for (const f of readdirSync(dir).filter((n) => n.endsWith('.test.mjs') && n !== 'lib-usage.test.mjs')) {
    const text = readFileSync(join(dir, f), 'utf8');
    if (PINS.test(text) && !/from '\.\/lib\/workflow\.mjs'/.test(text) && !LEGACY.has(f)) offenders.push(f);
  }
  assert.deepEqual(offenders, [], 'read the workflow with loadWorkflow()/loadAction() from ./lib/workflow.mjs and assert on fields (needs, if, with, outputs)');
});

test('the pattern sees the pins it guards against (probe)', () => {
  assert.ok(PINS.test("assert.match(job, /needs: \\[plan, gates\\]/);"));
  assert.ok(PINS.test("x.includes(`\\n    if: a == 'b'\\n`)"));
  assert.ok(PINS.test("assert.ok(lines.includes('train-skip=false'))"));
  assert.ok(!PINS.test("assert.equal(job.needs.length, 2)"));
});
