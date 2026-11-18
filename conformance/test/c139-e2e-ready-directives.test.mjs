import assert from 'node:assert/strict';
import { test } from 'node:test';
import { IMPLEMENTATION, parseAllow, readable, sites } from '../src/checks/C139-e2e-ready-directives.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const ALLOW = JSON.stringify({ allow: [{ path: 'ui/e2e/ok.spec.ts', site: 'paddock/e2e-ready-reads', reason: 'the read is immediate by design', count: 1 }] });
const D = '// eslint-disable-next-line paddock/e2e-ready-reads -- immediate by design\n';
const kinds = (path, text) => sites(path, text).map(({ line, kind }) => [line, kind]);

test('C139 sites: a one-line directive for the rule with a description is counted; the other forms are named', () => {
  assert.deepEqual(kinds('ui/e2e/a.spec.ts', `const a = 1;\n${D}`), [[2, 'counted']]);
  assert.deepEqual(kinds('ui/e2e/a.spec.ts', 'x(); // eslint-disable-line paddock/e2e-ready-reads -- why\n'), [[1, 'counted']]);
  assert.deepEqual(kinds('ui/e2e/a.spec.ts', '/* eslint-disable paddock/e2e-ready-reads */\n'), [[1, 'block']]);
  assert.deepEqual(kinds('ui/e2e/a.spec.ts', '/* eslint-disable */\n'), [[1, 'unrestricted']]);
  assert.deepEqual(kinds('ui/e2e/a.spec.ts', '// eslint-disable-next-line\n'), [[1, 'unrestricted']]);
  assert.deepEqual(kinds('ui/e2e/a.spec.ts', '// eslint-disable-next-line paddock/e2e-ready-reads\n'), [[1, 'no-reason']]);
  assert.deepEqual(kinds('ui/e2e/a.spec.ts', '// eslint-disable-next-line paddock/e2e-ready-reads -- ab\n'), [[1, 'no-reason']], 'a description is at least three characters');
  assert.deepEqual(kinds('ui/e2e/a.spec.ts', '// eslint-disable-next-line no-console, paddock/e2e-ready-reads -- why not\n'), [[1, 'counted']]);
  assert.deepEqual(kinds('ui/e2e/a.spec.ts', '// eslint-disable-next-line no-console -- another rule\n'), []);
  assert.deepEqual(kinds('ui/e2e/a.spec.ts', '// eslint-disable-next-line paddock/e2e-ready-reads -- \u{1F600}a\n'), [[1, 'counted']], 'an emoji is two UTF-16 units');
  assert.deepEqual(kinds('ui/e2e/a.spec.ts', '// eslint-disable-next-line paddock/e2e-ready-reads -- \u{1F600}\n'), [[1, 'no-reason']], 'an emoji alone (two UTF-16 units) is not three characters');
});

test('C139 readable: the e2e folders of an app and the ui only, not snapshots, node_modules or the check itself', () => {
  for (const p of ['ui/e2e/a.spec.ts', 'apps/dyno/app/e2e/a.spec.ts', 'apps/pitwall/app/e2e/screenshots/a.shot.ts']) assert.equal(readable(p), true, p);
  for (const p of ['ui/src/a.ts', 'apps/dyno/app/src/a.ts', 'ui/e2e/x-snapshots/a.ts', 'ui/e2e/node_modules/a.ts', 'ui/e2e/a.js', ...IMPLEMENTATION]) assert.equal(readable(p), false, p);
});

test('C139 allow list: a path, the rule as the site, a reason and a count', () => {
  assert.deepEqual(parseAllow(ALLOW), [{ path: 'ui/e2e/ok.spec.ts', site: 'paddock/e2e-ready-reads', reason: 'the read is immediate by design', count: 1 }]);
  assert.throws(() => parseAllow('{}'), /no `allow` array/);
});

test('C139 through the runner: the listed directive passes; an unlisted one, a second beyond the count, a block, a bare and a reasonless one are errors; a stale entry is a warning', async () => {
  const root = makeApp({
    'conformance/c139-allow.json': ALLOW,
    'ui/e2e/ok.spec.ts': `${D}a();\n${D}b();\n`,
    'ui/e2e/new.spec.ts': `${D}a();\n`,
    'ui/e2e/block.spec.ts': '/* eslint-disable paddock/e2e-ready-reads */\n',
    'ui/e2e/bare.spec.ts': '/* eslint-disable */\n',
    'ui/e2e/why.spec.ts': '// eslint-disable-next-line paddock/e2e-ready-reads\n',
    'ui/e2e/other.spec.ts': '// eslint-disable-next-line no-console -- not this rule\n',
  });
  const found = await findingsOf('C139', root);
  assert.deepEqual(found.map((f) => [f.file, f.line, f.severity]), [
    ['ui/e2e/bare.spec.ts', 1, 'error'],
    ['ui/e2e/block.spec.ts', 1, 'error'],
    ['ui/e2e/new.spec.ts', 1, 'error'],
    ['ui/e2e/ok.spec.ts', 3, 'error'],
    ['ui/e2e/why.spec.ts', 1, 'error'],
  ]);
  assert.match(found.find((f) => f.file === 'ui/e2e/ok.spec.ts').message, /beyond the count/);
  const stale = await findingsOf('C139', makeApp({ 'conformance/c139-allow.json': ALLOW, 'ui/e2e/ok.spec.ts': 'a();\n' }));
  assert.equal(stale.length, 1);
  assert.equal(stale[0].severity, 'warn');
  const broken = await findingsOf('C139', makeApp({ 'ui/e2e/a.spec.ts': D }));
  assert.equal(broken.length, 1);
  assert.equal(broken[0].file, 'conformance/c139-allow.json');
});
