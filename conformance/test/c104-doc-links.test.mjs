import assert from 'node:assert/strict';
import { test } from 'node:test';
import { added, maskCode, relativeLinks } from '../src/checks/C104-doc-links.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const nogit = { git: () => { throw new Error('no git'); } };
const base = { 'docs/rules.md': '# Rules\n', 'AGENTS.md': '# A\n' };

test('C104 reads relative links and skips schemes, anchors, absolute paths, code spans and fences', () => {
  const t = 'See [a](x/a.md#top), [b](https://e.example/b), [c](#here), [d](/abs), `[e](e.md)`, [f](<g.md>).\n```\n[z](z.md)\n```\n[m](mailto:a@b.c)\n';
  assert.deepEqual(relativeLinks(t).map((l) => [l.target, l.line]), [['x/a.md', 1], ['g.md', 1]]);
  assert.equal(maskCode('a `b` c').length, 7);
});

test('C104 fails a link whose file is missing, one error per link with file:line, and passes a good tree', async () => {
  const bad = makeApp({ ...base, 'docs/x.md': 'ok [r](rules.md)\nnow [gone](missing/hook.sh) and [dir](../docs)\n' });
  const f = await findingsOf('C104', bad, { options: nogit });
  assert.deepEqual(f.map((x) => [x.file, x.line, x.severity]), [['docs/x.md', 2, 'error']]);
  assert.match(f[0].message, /docs\/x\.md:2: .*missing\/hook\.sh/);
  assert.match(f[0].fix, /Fix the path/);
  const good = makeApp({ ...base, 'docs/x.md': '[r](rules.md) [a](../AGENTS.md)\n' });
  assert.deepEqual(await findingsOf('C104', good, { options: nogit }), []);
  assert.deepEqual(await findingsOf('C104', makeApp({ 'docs/x.md': '[g](gone.md)\n' }), { options: nogit }), [], 'a root without docs/rules.md is not checked');
});

test('C104 checks RULES.md given by option, against its own repository root', async () => {
  const rules = makeApp({ 'RULES.md': '[w](scripts/worktree.sh) [m](scripts/missing.sh)\n', 'scripts/worktree.sh': '#\n' });
  const f = await findingsOf('C104', makeApp(base), { options: { ...nogit, rulesMd: `${rules}/RULES.md` } });
  assert.deepEqual(f.map((x) => [x.file, x.line]), [['RULES.md', 1]]);
  assert.match(f[0].message, /scripts\/missing\.sh/);
});

test('C104 baseline: a listed dead link passes, a fixed one is a warning, a grown baseline is an error', async () => {
  const dead = { file: 'docs/x.md', target: 'gone.md' };
  const files = { ...base, 'docs/x.md': '[g](gone.md)\n', 'conformance/baselines/C104.json': JSON.stringify({ links: [dead] }) };
  const git = (args) => {
    if (args[0] === 'rev-parse') return 'x';
    if (args[0] === 'show') return JSON.stringify({ links: [] });
    return '';
  };
  const grew = await findingsOf('C104', makeApp(files), { options: { git } });
  assert.deepEqual(grew.map((x) => x.severity), ['error']);
  assert.match(grew[0].message, /baseline grew/);
  const same = await findingsOf('C104', makeApp(files), { options: { git: (a) => (a[0] === 'show' ? JSON.stringify({ links: [dead] }) : 'x') } });
  assert.deepEqual(same, []);
  const fixed = makeApp({ ...files, 'docs/x.md': 'fine\n' });
  const w = await findingsOf('C104', fixed, { options: { git: (a) => (a[0] === 'show' ? JSON.stringify({ links: [dead] }) : 'x') } });
  assert.deepEqual(w.map((x) => x.severity), ['warn']);
  assert.deepEqual(added([dead, dead], [dead]), [dead]);
  const intro = await findingsOf('C104', makeApp(files), { options: { git: (a) => { if (a[0] === 'show') throw new Error('absent'); return 'x'; } } });
  assert.deepEqual(intro, [], 'a baseline absent at the base is its introduction');
});

test('C104 named mechanisms: a missing ./dev command, hook, script or C-number is an error unless the sentence says planned', async () => {
  const cli = 'enum Command {\n    Wait(x),\n    Gates {\n    },\n}\n';
  const text = 'Run `./dev wait --max 5m` and `./dev disk record`. Use `scripts/gone.sh` and `.claude/hooks/gone.sh`; check C12 and C99.\nThe `./dev brief` is planned. See `scripts/here.sh`.\n';
  const root = makeApp({ ...base, 'AGENTS.md': text, 'tools/paddock-dev/src/cli.rs': cli, 'scripts/here.sh': '#\n', 'conformance/src/checks/C99-x.mjs': '//\n' });
  const f = await findingsOf('C104', root, { options: nogit });
  assert.deepEqual(f.map((x) => x.message.match(/`([^`]+)` is named/)?.[1]), ['./dev disk', 'scripts/gone.sh', '.claude/hooks/gone.sh', 'C12']);
  assert.ok(f.every((x) => x.file === 'AGENTS.md' && x.line === 1 && x.severity === 'error'));
  const ok = makeApp({ ...base, 'AGENTS.md': 'The `./dev brief` is planned.\n' });
  assert.deepEqual(await findingsOf('C104', ok, { options: nogit }), []);
});

test('C104 named mechanisms: baselined misses pass, an entry the merge base lacks is an error', async () => {
  const files = { ...base, 'AGENTS.md': 'Use `scripts/gone.sh`.\n', 'conformance/baselines/C104.json': JSON.stringify({ mentions: [{ file: 'AGENTS.md', target: 'scripts/gone.sh' }] }) };
  const at = (j) => ({ options: { git: (a) => (a[0] === 'show' ? JSON.stringify(j) : 'x') } });
  assert.deepEqual(await findingsOf('C104', makeApp(files), at({ mentions: [{ file: 'AGENTS.md', target: 'scripts/gone.sh' }] })), []);
  const grew = await findingsOf('C104', makeApp(files), at({ links: [] }));
  assert.match(grew[0].message, /baseline grew/);
});
