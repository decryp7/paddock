import assert from 'node:assert/strict';
import { test } from 'node:test';
import C143, { joinedProgramCalls } from '../src/checks/C143-spawn-joined-path.mjs';

const ctxOf = (files) => ({ files: () => Object.keys(files), read: (f) => files[f] ?? null });
const HEAD = "import { spawnSync, spawn, execFile, execFileSync, fork } from 'node:child_process';\nimport path, { join, resolve } from 'node:path';\n";
const hits = (body) => joinedProgramCalls(HEAD + body);

test('C143 bad: each spawn form with a relative join as the program', () => {
  assert.equal(hits("spawnSync(join('.', 'dev'), ['x']);\n").length, 1);
  assert.equal(hits("spawn(path.join('.', 'dev'));\n").length, 1);
  assert.equal(hits("execFile(path.join('scripts', 'x.sh'), []);\n").length, 1);
  assert.equal(hits("execFileSync(path.posix.join('..', 'dev'));\n").length, 1);
  assert.equal(hits("fork(join('./tools', 'a.mjs'));\n").length, 1);
  assert.equal(hits("spawnSync(\n  join(\n    '.',\n    'dev',\n  ),\n  [],\n);\n")[0].line, 3);
  assert.equal(hits("cp.spawn(join('', 'dev'));\n").length, 1);
});

test('C143 bad: a variable assigned from a relative join', () => {
  const found = hits("const dev = join('.', 'dev');\nspawnSync(dev, ['x']);\n");
  assert.equal(found.length, 1);
  assert.equal(found[0].line, 4);
  assert.match(found[0].message, /assigned on line 3/);
  assert.equal(hits("let p = path.join('bin', 'x');\nspawn(p);\n").length, 1);
});

test('C143 good: resolve, absolute parts, expressions, plain strings and other first arguments', () => {
  assert.deepEqual(hits("spawnSync(resolve('.', 'dev'), []);\n"), []);
  assert.deepEqual(hits("spawnSync(join('/usr', 'bin', 'x'), []);\n"), []);
  assert.deepEqual(hits("spawnSync(join('C:\\\\tools', 'x.exe'), []);\n"), []);
  assert.deepEqual(hits("spawnSync(join(root, 'dev'), []);\n"), []);
  assert.deepEqual(hits("spawnSync(join(__dirname, 'dev'), []);\n"), []);
  assert.deepEqual(hits("spawnSync('git', [join('.', 'x')]);\n"), [], 'a join in the arguments is not the program');
  assert.deepEqual(hits("spawnSync(process.execPath, [join('.', 'x.mjs')]);\n"), []);
  assert.deepEqual(hits("const dev = resolve('.', 'dev');\nspawnSync(dev, []);\n"), []);
  assert.deepEqual(hits("const dev = join(root, 'dev');\nspawnSync(dev, []);\n"), []);
});

test('C143 not read: comments, strings, declarations, array joins and files without child_process', () => {
  assert.deepEqual(hits("// spawnSync(join('.', 'dev'));\n"), []);
  assert.deepEqual(hits("const s = \"spawnSync(join('.', 'dev'))\";\n"), []);
  assert.deepEqual(hits("function spawn(join) { return 1; }\n"), []);
  assert.deepEqual(hits("spawn(['.', 'dev'].join('/'));\n"), []);
  assert.deepEqual(joinedProgramCalls("spawnSync(join('.', 'dev'));\n"), [], 'no child_process in the file');
});

test('C143 run: findings carry file and line; non-source files are skipped', () => {
  const found = C143.run(ctxOf({ 'scripts/a.mjs': `${HEAD}spawnSync(join('.', 'dev'));\n`, 'docs/a.md': `${HEAD}spawnSync(join('.', 'dev'));\n`, 'x/node_modules/b.mjs': `${HEAD}spawn(join('.', 'x'));\n` }));
  assert.equal(found.length, 1);
  assert.equal(found[0].file, 'scripts/a.mjs');
  assert.equal(found[0].line, 3);
  assert.equal(found[0].severity, 'warn');
  assert.match(found[0].fix, /resolve/);
});
