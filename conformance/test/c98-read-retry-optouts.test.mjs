import assert from 'node:assert/strict';
import { test } from 'node:test';
import { temporaryOptOuts } from '../src/checks/C98-read-retry-optouts.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const STORE = 'ui/projects/paddock-ui/shell/x/src/x.store.ts';
const call = (body) => `export const XStore = signalStore(\n  withState({}),\n  withRequestStatus(${body}),\n);\n`;
const now = new Date('2026-11-08T12:00:00Z');

test('C98 reads the temporary opt-outs of a file with their dates and lines', () => {
  const text = `${call("{ retry: true }")}${call("{\n    retry: false,\n    kind: 'temporary',\n    why: 'not adopted yet, see the ADR',\n    until: '2026-12-31',\n  }")}${call("{ retry: false, kind: 'permanent', why: 'one search per query' }")}`;
  assert.deepEqual(temporaryOptOuts(text), [{ line: 7, until: '2026-12-31' }]);
  assert.deepEqual(temporaryOptOuts(call("{ retry: false, kind: 'temporary', why: 'later' }")), [{ line: 3, until: null }]);
});

test('C98 through the runner: a passed or missing date is an error; a date to come, a permanent opt-out and specs are not', async () => {
  const root = makeApp({
    [STORE]: call("{ retry: false, kind: 'temporary', why: 'not adopted yet', until: '2026-11-07' }"),
    'ui/projects/paddock-ui/live/src/y.store.ts': call("{ retry: false, kind: 'temporary', why: 'not adopted yet' }"),
    'ui/projects/paddock-ui/view/src/z.store.ts': call("{ retry: false, kind: 'temporary', why: 'not adopted yet', until: '2026-11-08' }"),
    'ui/projects/paddock-ui/view/src/p.store.ts': call("{ retry: false, kind: 'permanent', why: 'a validation' }"),
    'ui/projects/paddock-ui/view/src/z.spec.ts': call("{ retry: false, kind: 'temporary', why: 'a fixture', until: '2000-01-01' }"),
  });
  const f = await findingsOf('C98', root, { options: { now } });
  assert.deepEqual(
    f.map((x) => [x.file, x.line, x.severity]),
    [
      ['ui/projects/paddock-ui/live/src/y.store.ts', 3, 'error'],
      [STORE, 3, 'error'],
    ],
  );
  assert.match(f[1].message, /2026-11-07 has passed/);
  assert.match(f[0].message, /no `until` date/);
});

test('C98 finds nothing outside the shared UI', async () => {
  const root = makeApp({ 'apps/demo/app/src/a.store.ts': call("{ retry: false, kind: 'temporary', why: 'x', until: '2000-01-01' }") });
  assert.deepEqual(await findingsOf('C98', root, { options: { now } }), []);
});
