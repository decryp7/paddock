import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { blockFiles, findingsOf, makeApp, messages, PADDOCK } from './helpers.mjs';
import { findBlock, findSection, inspectBlock, loadBlocks, renderRules, revOf } from '../src/sync/blocks.mjs';
import { main } from '../src/cli.mjs';

const run = (files, extra = {}) => findingsOf('C23', makeApp({ ...files, ...extra }), { options: { paddock: PADDOCK } }).then((f) => f.filter((x) => !x.file.startsWith('.git')));

test('C23 blocks good: every default block in place, app sections untouched', async () => {
  assert.deepEqual(await run(blockFiles()), []);
});

test('C23 blocks bad: missing, unmarked, hand-edited, outdated and broken', async () => {
  const f = blockFiles();
  let agents = f['AGENTS.md'];
  agents = agents.replace('<!-- paddock:block style rev=' , '<!-- stripped style rev=').replace('<!-- /paddock:block style -->', '');
  agents = agents.replace('## Working practices', '## Working practices\n\nHand-added line.');
  agents = agents.replace(/design-principles rev=[0-9a-f]{8}/, 'design-principles rev=00000000');
  agents = agents.replace('<!-- /paddock:block where-logic-lives -->', '');
  f['AGENTS.md'] = agents;
  delete f['docs/releasing.md'];
  const m = messages(await run(f));
  assert.match(m, /"working-practices" was edited by hand/);
  assert.match(m, /"design-principles" is outdated/);
  assert.match(m, /"where-logic-lives" are unmatched|markers of the shared block "where-logic-lives" are unmatched/);
  assert.match(m, /"style" is not between paddock:block markers/);
});

test('C23 blocks: an app without docs/releasing.md or suite-lessons is not forced to have them; AGENTS.md is', async () => {
  const f = blockFiles();
  delete f['docs/releasing.md'];
  delete f['docs/suite-lessons.md'];
  assert.deepEqual(await run(f), []);
  const m = messages(await run({ 'README.md': 'x' }));
  assert.match(m, /AGENTS\.md: the shared block "rules" is missing/);
});

test('C23 blocks: docBlocks narrows or extends the targets; an unknown name is a crash finding', async () => {
  const f = blockFiles();
  f['paddock.conformance.json'] = JSON.stringify({ docBlocks: { 'AGENTS.md': ['rules'] } });
  assert.deepEqual(await run(f), []);
  f['paddock.conformance.json'] = JSON.stringify({ docBlocks: { 'AGENTS.md': ['nope'] } });
  assert.match(messages(await run(f)), /unknown block "nope"/);
});

test('C23 blocks are quiet without --paddock (C28 warns about it)', async () => {
  assert.deepEqual((await findingsOf('C23', makeApp(blockFiles()), { options: {} })).filter((x) => !x.file.startsWith('.git')), []);
});

test('the rules block is docs/rules.md, one level down, links absolute, and carries the app-facing intro', () => {
  const md = renderRules(readFileSync(join(PADDOCK, 'docs', 'rules.md'), 'utf8'));
  assert.match(md, /^## Paddock rules \(binding\)\n\nThe family's binding rules, copied from \[paddock\/docs\/rules\.md\]/);
  assert.match(md, /\n### Way of working\n1\. \*\*One release at a time/);
  assert.doesNotMatch(md, /\n## Way of working/);
  assert.match(md, /\]\(https:\/\/dev\.decryptology\.net\/decryp7\/paddock\/src\/branch\/main\/docs\/research\//);
  assert.throws(() => renderRules('# T\n\nno sections'), /no "## " section/);
});

test('inspectBlock: sections end at the next heading, fences are respected, adjacent blocks do not swallow each other', () => {
  const reg = loadBlocks(PADDOCK);
  const style = reg.blocks.find((b) => b.name === 'style');
  const text = '# T\n\n## Style\n\nold\n\n```sh\n# comment in a fence\n```\n\nmore old\n\n## Next\n\nkeep\n';
  const r = inspectBlock(text, style, { app: 'demo' });
  assert.equal(r.state, 'unmarked');
  assert.match(r.desired, /## Next\n\nkeep/);
  assert.doesNotMatch(r.desired, /more old/);
  assert.deepEqual(findSection(text, '## Style'), { from: 2, to: 12 });
  assert.equal(findSection(text, '## Absent'), null);
  assert.equal(inspectBlock(r.desired, style, { app: 'demo' }).state, 'ok');
  assert.equal(findBlock('no markers', 'style'), null);
  assert.ok(revOf('a') !== revOf('b'));
});

test('first-launch fills the app name; a whole-file block replaces a hand-written file', () => {
  const reg = loadBlocks(PADDOCK);
  const fl = reg.blocks.find((b) => b.name === 'first-launch');
  assert.match(inspectBlock('', fl, { app: 'pitwall', appName: 'Pitwall' }).desired, /Pitwall is not code-signed yet/);
  const sl = reg.blocks.find((b) => b.name === 'suite-lessons');
  const r = inspectBlock('# Old hand-written lessons\n', sl, { app: 'x' });
  assert.equal(r.state, 'unmarked');
  assert.doesNotMatch(r.desired, /Old hand-written/);
  assert.equal(inspectBlock('', sl, { app: 'x' }).state, 'missing');
});

test('sync --write adopts an app: unmarked sections get markers, own sections stay', async () => {
  const root = makeApp({
    'AGENTS.md': '# Demo\n\n## Design principles\n\nold words\n\n## Own section\n\nmine\n\n## Style\n\nold style\n',
    'docs/suite-lessons.md': '# old\n',
  });
  const io = { log() {}, err() {} };
  const argv = ['sync', '--app', 'demo', '--root', root, '--paddock', PADDOCK];
  await main([...argv, '--write'], io);
  const out = readFileSync(join(root, 'AGENTS.md'), 'utf8');
  assert.match(out, /## Own section\n\nmine/);
  assert.doesNotMatch(out, /old words/);
  assert.match(out, /<!-- paddock:block rules rev=/);
  assert.equal(await main(argv, io), 0);
});
