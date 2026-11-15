import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { test } from 'node:test';
import { CONTRACT_PATH, INDEX_PATH, REGENERATE, contractText, generate, lineDiff, modules } from '../src/derived-lists.mjs';
import { makeApp } from './helpers.mjs';

const ROOT = resolve(import.meta.dirname, '..', '..');
const BIN = join(ROOT, 'conformance/bin/paddock-check-lists.mjs');
const contract = (skip, extra = {}) => JSON.stringify({ conformance: { app: 'paddock', root: '.', skip, only: [], strict: [], ...extra } }, null, 2);

test('the committed index.mjs and paddock.ci.json `only` are what the check modules give', () => {
  const files = readdirSync(join(ROOT, 'conformance/src/checks'));
  for (const { path, text } of generate(files, readFileSync(join(ROOT, CONTRACT_PATH), 'utf8'))) {
    const have = readFileSync(join(ROOT, path), 'utf8');
    assert.equal(have, text, `${path} is stale: run \`${REGENERATE}\` and commit the result\n${lineDiff(have, text).join('\n')}`);
  }
});

test('a new module with no other edit is imported, listed and run on Paddock after the regenerate', () => {
  const files = readdirSync(join(ROOT, 'conformance/src/checks'));
  const [index, ci] = generate([...files, 'C99999-fake.mjs'], readFileSync(join(ROOT, CONTRACT_PATH), 'utf8'));
  assert.equal(index.path, INDEX_PATH);
  assert.match(index.text, /^import C99999 from '\.\/C99999-fake\.mjs';$/m);
  assert.match(index.text, /^export const checks = \[C03, .*, C99999\];$/m);
  assert.equal(JSON.parse(ci.text).conformance.only.at(-1), 'C99999');
});

test('modules: id order by number, other files ignored, two modules with one id refused', () => {
  assert.deepEqual(
    modules(['index.mjs', 'lib', 'C104-b.mjs', 'C99-a.mjs', 'C3-c.mjs', 'C5.mjs', 'notes.md']).map((m) => m.id),
    ['C3', 'C99', 'C104'],
  );
  assert.throws(() => modules(['C7-a.mjs', 'C7-b.mjs']), /C7 has two modules \(C7-a\.mjs and C7-b\.mjs\)/);
});

test('contract: only is every module less skip; a stale, unreasoned or strict-but-skipped entry is refused', () => {
  const mods = modules(['C1-a.mjs', 'C2-b.mjs', 'C3-c.mjs']);
  assert.deepEqual(JSON.parse(contractText(contract({ C2: 'app-only' }), mods)).conformance.only, ['C1', 'C3']);
  assert.throws(() => contractText(contract({ C9: 'gone' }), mods), /names C9, which has no module/);
  assert.throws(() => contractText(contract({ C2: ' ' }), mods), /skip\.C2` needs the reason/);
  assert.throws(() => contractText(contract({ C2: 'x' }, { strict: ['C2'] }), mods), /strict` names C2, which does not run/);
  assert.throws(() => contractText(JSON.stringify({ conformance: { only: [] } }), mods), /`conformance\.skip` must be an object/);
});

const scratchRepo = () => makeApp({ 'conformance/src/checks/C1-one.mjs': "export default { id: 'C1' };\n", [CONTRACT_PATH]: `${contract({})}\n` });

const run = (root, ...args) => spawnSync(process.execPath, [BIN, '--root', root, ...args], { encoding: 'utf8' });

test('the command: --check names the stale file, the lines and the command; a run rewrites both and --check then passes', () => {
  const root = scratchRepo();
  try {
    assert.equal(run(root).status, 0);
    assert.equal(run(root, '--check').status, 0);
    writeFileSync(join(root, 'conformance/src/checks/C999-fake.mjs'), "export default { id: 'C999' };\n");
    const stale = run(root, '--check');
    assert.equal(stale.status, 1);
    assert.match(stale.stderr, /conformance\/src\/checks\/index\.mjs is stale: run `node conformance\/bin\/paddock-check-lists\.mjs`/);
    assert.match(stale.stderr, /\+ import C999 from '\.\/C999-fake\.mjs';/);
    assert.match(stale.stderr, /conformance\/paddock\.ci\.json is stale/);
    assert.equal(run(root).status, 0);
    assert.equal(run(root, '--check').status, 0);
    assert.deepEqual(JSON.parse(readFileSync(join(root, CONTRACT_PATH), 'utf8')).conformance.only, ['C1', 'C999']);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('the command refuses a module whose exported id is not the one in its file name', () => {
  const root = scratchRepo();
  try {
    writeFileSync(join(root, 'conformance/src/checks/C2-two.mjs'), "export default { id: 'C3' };\n");
    const r = run(root, '--check');
    assert.equal(r.status, 1);
    assert.match(r.stderr, /C2-two\.mjs exports id "C3", not C2/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
