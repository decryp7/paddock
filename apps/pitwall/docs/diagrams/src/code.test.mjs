// Doc facts checked from code: every code name the Pitwall diagrams draw (class boxes, their operations and fields, nodes,
// file and crate names, calls in sequence messages) exists in the sources. The helper is shared (diagrams/tools/code-check.mjs).
// Run: node --test docs/diagrams/src/*.test.mjs (from apps/pitwall; part of the app's checks in paddock.ci.json).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { join } from 'node:path';
import { PANELS } from './c43.mjs';
import { indexCode, recordDiagrams, unknownCode } from '../../../../../diagrams/tools/code-check.mjs';

const here = new URL('.', import.meta.url).pathname;
const app = join(here, '../../..');
const rec = await recordDiagrams(here, ['c4', 'cls', 'seq']);
// c43.mjs records its panels in PANELS and writes only when run, so its nodes join the recording here.
for (const [file, nodes] of Object.entries(PANELS)) rec[file] = { nodes };
const code = indexCode([join(app, 'crates'), join(app, 'app/src'), join(app, 'app/src-tauri')], { crateRoots: [join(app, '../../crates'), join(app, '../../ui/projects')] });

/** Names that are prose on purpose, each with its reason. */
const SKIP = {};

test('the probes see the diagrams and the code', () => {
  for (const f of ['uml-class-serving', 'c4-3-core-wire', 'c4-1-context']) assert.ok(rec[f], `${f} was not recorded`);
  assert.ok(code.types.has('Serving') && code.fns.has('resolve'), 'the code index does not see pitwall-server');
});

test('every code name the diagrams draw exists in the code', () => {
  assert.deepEqual(unknownCode(rec, code, { skip: SKIP }), [], 'fix the diagram source (docs/diagrams/src) or the name; SKIP only prose, with a reason');
});

test('the check fails on a name the code does not have (mutation)', () => {
  const fake = { x: { nodes: [{ name: 'NoSuchThing', tech: 'src/nothing_here.rs' }], classes: [{ name: 'Ghost', attrs: ['zzz_field: u8'], ops: ['zzz_op() → ()'] }], messages: ['zzz_call(x)'] } };
  assert.equal(unknownCode(fake, code).length, 6);
});
