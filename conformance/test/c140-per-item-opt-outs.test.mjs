import assert from 'node:assert/strict';
import { test } from 'node:test';
import { optOuts, parseAllow } from '../src/checks/C140-per-item-opt-outs.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const ALLOW = 'conformance/c140-allow.json';
const list = (allow = []) => JSON.stringify({ allow });
const entry = (path, count = 1) => ({ path, site: 'per-item', reason: 'each runner answers on its own pipe', count });

test('C140 finds every per-item opt-out with its line and reason, and the ones whose reason is under three words', () => {
  const text = 'a();\n// per-item: each runner has its own pipe\nfor (const r of rs) await backend.ping(r);\n/* per-item: legacy */\nx();\n// per-items: not the tag\n';
  assert.deepEqual(optOuts(text), [
    { line: 2, reason: 'each runner has its own pipe', short: false },
    { line: 4, reason: 'legacy', short: true },
  ]);
  assert.deepEqual(optOuts("const s = 'per-item: in a string';\n"), []);
});

test('C140 allow file: a path, the site per-item, a reason and a count', () => {
  assert.deepEqual(parseAllow(list([entry('a.ts')])), [entry('a.ts')]);
  assert.throws(() => parseAllow(list([{ path: 'a.ts', reason: 'long enough' }])), /site/);
});

test('C140 through the runner: listed opt-outs pass, unlisted ones and short reasons are errors', async () => {
  const files = {
    [ALLOW]: list([entry('apps/a/app/src/listed.store.ts')]),
    'apps/a/app/src/listed.store.ts': '// per-item: each runner answers on its own pipe\nfor (const r of rs) await backend.ping(r);\n',
    'apps/a/app/src/new.store.ts': '// per-item: the helper has no bulk form yet\nfor (const c of cs) await backend.wol(c);\n',
    'apps/a/app/src/short.store.ts': '// per-item: legacy\nfor (const c of cs) await backend.wol(c);\n',
    'apps/a/app/src/x.spec.ts': '// per-item: a spec plants the shape on purpose\n',
    'ui/projects/paddock-ui/src/fine.ts': 'await backend.all(ids);\n',
  };
  const base = (text) => ({ git: (args) => (args[0] === 'show' ? text : 'abc123') });
  const same = await findingsOf('C140', makeApp(files), { options: base(list([entry('apps/a/app/src/listed.store.ts')])) });
  assert.deepEqual(same.map((f) => [f.file, f.line, f.severity]).sort(), [
    ['apps/a/app/src/new.store.ts', 1, 'error'],
    ['apps/a/app/src/short.store.ts', 1, 'error'],
    ['apps/a/app/src/short.store.ts', 1, 'error'],
  ]);
  // Growth against the merge base is the shared allow list's own finding, tested once with C140's sample in allow-list.test.mjs (ER-22).
});

test('C140 positive control (ER-33): with the opt-out taken away from the text there is nothing to count', async () => {
  const found = await findingsOf('C140', makeApp({ [ALLOW]: list(), 'apps/a/app/src/a.ts': 'for (const c of cs) await backend.wol(c);\n' }));
  assert.deepEqual(found, []);
  const back = await findingsOf('C140', makeApp({ [ALLOW]: list(), 'apps/a/app/src/a.ts': '// per-item: the helper has no bulk form yet\nfor (const c of cs) await backend.wol(c);\n' }));
  assert.equal(back.length, 1);
});

test('C140 a broken allow file is one finding, nothing else', async () => {
  for (const text of [null, 'nope']) {
    const found = await findingsOf('C140', makeApp({ 'apps/a/app/src/x.ts': '// per-item: one two three\n', ...(text === null ? {} : { [ALLOW]: text }) }));
    assert.deepEqual(found.map((f) => [f.file, f.outcome]), [[ALLOW, 'check-broken']]);
  }
});
