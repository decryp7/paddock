import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { test } from 'node:test';
import { checks } from '../src/checks/index.mjs';
import {
  CONTRACT_PATH,
  INDEX_PATH,
  README_BEGIN,
  README_END,
  README_PATH,
  REGENERATE,
  contractText,
  generate,
  lineDiff,
  modules,
  readmeText,
} from '../src/derived-lists.mjs';
import { makeApp } from './helpers.mjs';

const ROOT = resolve(import.meta.dirname, '..', '..');
const BIN = join(ROOT, 'conformance/bin/paddock-check-lists.mjs');
const CHECKS = join(ROOT, 'conformance/src/checks');
const contract = (skip, extra = {}) => JSON.stringify({ conformance: { app: 'paddock', root: '.', skip, only: [], strict: [], ...extra } }, null, 2);
const README = `# x\n\n${README_BEGIN}\n${README_END}\n\nmore\n`;
const files = () =>
  readdirSync(CHECKS, { withFileTypes: true })
    .filter((e) => !e.isDirectory())
    .map((e) => e.name);
const metas = checks.map((c) => ({ id: c.id, title: c.title, scope: c.scope, mode: c.mode, needs: c.needs }));
const read = (p) => readFileSync(join(ROOT, p), 'utf8');

test('the committed index.mjs, paddock.ci.json `only` and README index are what the check modules give', () => {
  for (const { path, text } of generate(files(), read(CONTRACT_PATH), read(README_PATH), metas)) {
    const have = read(path);
    assert.equal(have, text, `${path} is stale: run \`${REGENERATE}\` and commit the result\n${lineDiff(have, text).join('\n')}`);
  }
});

test('a new module with no other edit is imported, listed, run on Paddock and indexed after the regenerate', () => {
  const fake = { id: 'C99999', title: 'A fake check', scope: 'root', mode: 'report-only', needs: 'all' };
  const [index, ci, readme] = generate([...files(), 'C99999-fake.mjs'], read(CONTRACT_PATH), read(README_PATH), [...metas, fake]);
  assert.equal(index.path, INDEX_PATH);
  assert.match(index.text, /^import C99999 from '\.\/C99999-fake\.mjs';$/m);
  assert.match(index.text, /^export const checks = \[C03, .*, C99999\];$/m);
  assert.equal(JSON.parse(ci.text).conformance.only.at(-1), 'C99999');
  assert.match(readme.text, /^\| C99999 \| A fake check \| root \| report-only \| all \|$/m);
});

test('modules: id order by number; two modules with one id refused', () => {
  assert.deepEqual(
    modules(['index.mjs', 'C104-b.mjs', 'C99-a.mjs', 'C3-c.mjs']).map((m) => m.id),
    ['C3', 'C99', 'C104'],
  );
  assert.throws(() => modules(['C7-a.mjs', 'C7-b.mjs']), /C7 has two modules \(C7-a\.mjs and C7-b\.mjs\)/);
});

test('modules: a misnamed file in checks/ is refused with the expected shape, never skipped (its check would never run)', () => {
  for (const name of ['C127-Foo.mjs', 'C127_foo.mjs', 'c127-foo.mjs', 'C127-foo.js', 'C127.mjs', 'notes.md']) {
    assert.throws(() => modules(['index.mjs', 'C1-a.mjs', name]), (e) => e.message.includes(`${name} is not a check module: name it C<n>-<slug>.mjs`), name);
  }
});

test('contract: only is every module less skip; a stale, unreasoned or strict-but-skipped entry is refused', () => {
  const mods = modules(['C1-a.mjs', 'C2-b.mjs', 'C3-c.mjs']);
  assert.deepEqual(JSON.parse(contractText(contract({ C2: 'app-only' }), mods)).conformance.only, ['C1', 'C3']);
  assert.throws(() => contractText(contract({ C9: 'gone' }), mods), /names C9, which has no module/);
  assert.throws(() => contractText(contract({ C2: ' ' }), mods), /skip\.C2` needs the reason/);
  assert.throws(() => contractText(contract({ C2: 'x' }, { strict: ['C2'] }), mods), /strict` names C2, which does not run/);
  assert.throws(() => contractText(JSON.stringify({ conformance: { only: [] } }), mods), /`conformance\.skip` must be an object/);
});

test('readme: the index between the markers is rewritten, the rest kept; a pipe in a title is escaped; no markers is refused', () => {
  const out = readmeText(README, [{ id: 'C1', title: 'a | b', scope: 'root', mode: 'gate' }]);
  assert.equal(out, `# x\n\n${README_BEGIN}\n| Id | Title | Scope | Mode | Needs |\n|---|---|---|---|---|\n| C1 | a \\| b | root | gate | all |\n${README_END}\n\nmore\n`);
  assert.equal(readmeText(out, [{ id: 'C1', title: 'a | b', scope: 'root', mode: 'gate' }]), out);
  assert.throws(() => readmeText('# x\n', []), /needs the lines/);
});

test('lineDiff: the lines to add and remove; same lines in another order name the first position that differs', () => {
  assert.deepEqual(lineDiff('a\nb\n', 'a\nc\n'), ['+ c', '- b']);
  assert.deepEqual(lineDiff('a\nb\n', 'a\nb\n'), []);
  assert.deepEqual(lineDiff('b\na\n', 'a\nb\n'), ['line 1 (same lines, other order or count): + a', 'line 1: - b']);
});

const scratchRepo = () =>
  makeApp({ 'conformance/src/checks/C1-one.mjs': "export default { id: 'C1', title: 'One', scope: 'root', mode: 'gate' };\n", [CONTRACT_PATH]: `${contract({})}\n`, [README_PATH]: README });

const env = (extra = {}) => {
  const e = { ...process.env, ...extra };
  if (!('CI' in extra)) delete e.CI;
  return e;
};
const run = (root, args = [], extra = {}) => spawnSync(process.execPath, [BIN, '--root', root, ...args], { encoding: 'utf8', env: env(extra) });

test('the command: --check names the stale file, the lines and the command; a run rewrites them, prints the change, and --check passes', () => {
  const root = scratchRepo();
  try {
    assert.equal(run(root).status, 0);
    assert.equal(run(root, ['--check']).status, 0);
    writeFileSync(join(root, 'conformance/src/checks/C999-fake.mjs'), "export default { id: 'C999', title: 'Fake', scope: 'root', mode: 'gate' };\n");
    const stale = run(root, ['--check']);
    assert.equal(stale.status, 1);
    assert.match(stale.stderr, /conformance\/src\/checks\/index\.mjs is stale: run `node conformance\/bin\/paddock-check-lists\.mjs`/);
    assert.match(stale.stderr, /\+ import C999 from '\.\/C999-fake\.mjs';/);
    assert.match(stale.stderr, /conformance\/paddock\.ci\.json is stale/);
    assert.match(stale.stderr, /conformance\/README\.md is stale/);
    const wrote = run(root);
    assert.equal(wrote.status, 0);
    assert.match(wrote.stdout, /wrote conformance\/README\.md\n {2}\+ \| C999 \| Fake \| root \| gate \| all \|/);
    assert.equal(run(root, ['--check']).status, 0);
    assert.deepEqual(JSON.parse(readFileSync(join(root, CONTRACT_PATH), 'utf8')).conformance.only, ['C1', 'C999']);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('the command refuses a misnamed module file and one whose exported id is not the one in its file name; folders are not modules', () => {
  const root = scratchRepo();
  try {
    mkdirSync(join(root, 'conformance/src/checks/lib'));
    assert.equal(run(root).status, 0);
    assert.equal(run(root, ['--check']).status, 0, 'a folder (lib/) is no module');
    writeFileSync(join(root, 'conformance/src/checks/C2-two.mjs'), "export default { id: 'C3' };\n");
    const r = run(root, ['--check']);
    assert.equal(r.status, 1);
    assert.match(r.stderr, /C2-two\.mjs exports id "C3", not C2/);
    rmSync(join(root, 'conformance/src/checks/C2-two.mjs'));
    writeFileSync(join(root, 'conformance/src/checks/C127_foo.mjs'), "export default { id: 'C127' };\n");
    const m = run(root, ['--check']);
    assert.equal(m.status, 1);
    assert.match(m.stderr, /C127_foo\.mjs is not a check module/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('the command: --root without a folder and an unknown argument are usage errors; a rewrite under CI=true is refused', () => {
  const root = scratchRepo();
  try {
    const bare = spawnSync(process.execPath, [BIN, '--root'], { encoding: 'utf8', env: env() });
    assert.equal(bare.status, 2);
    assert.match(bare.stderr, /--root needs a folder/);
    assert.equal(spawnSync(process.execPath, [BIN, '--root', '--check'], { encoding: 'utf8', env: env() }).status, 2);
    assert.match(run(root, ['--nope']).stderr, /unknown argument --nope/);
    const ci = run(root, [], { CI: 'true' });
    assert.equal(ci.status, 2);
    assert.match(ci.stderr, /CI=true: CI checks the lists/);
    assert.equal(run(root).status, 0);
    assert.equal(run(root, ['--check'], { CI: 'true' }).status, 0, '--check runs in CI');
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('every row of the README notes table names a check that exists (no stale rows; the table need not be complete)', () => {
  const text = read(README_PATH);
  const notes = text.slice(text.indexOf(README_END));
  const ids = new Set(checks.map((c) => c.id));
  const rows = [...notes.matchAll(/^\| (C\d+) \|/gm)].map((m) => m[1]);
  assert.ok(rows.length > 0, 'the notes table has rows: the probe sees them');
  const stale = rows.filter((id) => !ids.has(id));
  assert.deepEqual(stale, [], `conformance/README.md notes rows for checks with no module: ${stale.join(', ')}; remove them`);
});
