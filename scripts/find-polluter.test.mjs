import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { findPolluter, parseArgs } from './find-polluter.mjs';
import { orderedSequence, orderFiles, parseOrder } from './ordered-sequencer.mjs';

const SCRIPT = fileURLToPath(new URL('./find-polluter.mjs', import.meta.url));
const files = Array.from({ length: 200 }, (_, i) => `spec${i}.ts`);

test('finds one polluter among 200 files in about log2(n) + 2 runs', () => {
  for (const polluter of ['spec0.ts', 'spec77.ts', 'spec199.ts']) {
    const r = findPolluter(files, (s) => s.includes(polluter));
    assert.deepEqual([r.status, r.polluter], ['found', polluter]);
    assert.ok(r.runs <= 2 * Math.ceil(Math.log2(200)) + 2, `${r.runs} runs`);
  }
});

test('reports a victim that passes, one that fails alone, and one that needs two files', () => {
  assert.equal(findPolluter(files, () => false).status, 'no-repro');
  assert.equal(findPolluter(files, () => true).status, 'fails-alone');
  const both = findPolluter(files, (s) => s.includes('spec3.ts') && s.includes('spec150.ts'));
  assert.equal(both.status, 'combination');
  assert.ok(both.remaining.includes('spec3.ts') || both.remaining.includes('spec150.ts'));
});

test('parseArgs requires victim, list and command', () => {
  assert.ok(parseArgs(['--victim', 'a']).error);
  assert.deepEqual(parseArgs(['--victim', 'v', '--before', 'b', '--cwd', '/x', '--', 'npx', 'vitest']), {
    victim: 'v', before: 'b', cwd: '/x', timeout: 600000, repeat: 1, noFlags: false, fileFlag: undefined, cmd: ['npx', 'vitest'],
  });
  const opts = parseArgs(['--victim', 'v', '--before', 'b', '--timeout', '5', '--repeat', '3', '--no-flags', '--file-flag', '--include', '--', 'ng']);
  assert.deepEqual([opts.timeout, opts.repeat, opts.noFlags, opts.fileFlag], [5, 3, true, '--include']);
  assert.ok(parseArgs(['--victim', 'v', '--before', 'b', '--repeat', '0', '--', 'x']).error);
});

test('command line: spawns the command per subset and prints the polluter', () => {
  const dir = mkdtempSync(join(tmpdir(), 'polluter-'));
  try {
    // The fake test command fails (exit 1) when it is given pollute.spec.ts, as a leaking spec would.
    const runner = join(dir, 'fake-test.mjs');
    writeFileSync(runner, "process.exit(process.argv.slice(2).some((a) => a.endsWith('pollute.spec.ts')) ? 1 : 0);\n");
    const list = join(dir, 'before.txt');
    const names = Array.from({ length: 20 }, (_, i) => (i === 13 ? 'pollute.spec.ts' : `ok${i}.spec.ts`));
    writeFileSync(list, `${names.join('\n')}\nvictim.spec.ts\n`);
    const r = spawnSync(process.execPath, [SCRIPT, '--victim', 'victim.spec.ts', '--before', list, '--', process.execPath, runner], {
      encoding: 'utf8',
    });
    assert.equal(r.status, 0, r.stderr);
    assert.match(r.stdout, /polluter: \S*pollute\.spec\.ts/);
    const bad = spawnSync(process.execPath, [SCRIPT], { encoding: 'utf8' });
    assert.equal(bad.status, 2);
    assert.match(bad.stderr, /::error::usage/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('ordered sequencer keeps the listed order and puts unlisted files last', () => {
  const files = ['/c', '/a', '/z', '/b'].map((moduleId) => ({ moduleId }));
  assert.deepEqual(orderFiles(files, ['/a', '/b', '/c']).map((f) => f.moduleId), ['/a', '/b', '/c', '/z']);
  assert.deepEqual(parseOrder('/a\n\n /b \n'), ['/a', '/b']);
  assert.deepEqual(orderedSequence({}), {});
  assert.equal(orderedSequence({ PADDOCK_TEST_ORDER: '/a' }).shuffle, false);
});

test('a flag for the file argument and a timeout are applied', () => {
  const dir = mkdtempSync(join(tmpdir(), 'polluter-flags-'));
  try {
    const runner = join(dir, 'fake.mjs');
    // Fails when --include is followed by a path ending in pollute.ts; hangs never; records nothing else.
    writeFileSync(runner, "const a = process.argv.slice(2); process.exit(a.some((x, i) => a[i - 1] === '--include' && x.endsWith('pollute.ts')) ? 1 : 0);\n");
    const list = join(dir, 'before.txt');
    writeFileSync(list, 'a.ts\npollute.ts\nb.ts\n');
    const r = spawnSync(process.execPath, [SCRIPT, '--victim', 'v.ts', '--before', list, '--cwd', dir, '--no-flags', '--file-flag', '--include', '--repeat', '2', '--timeout', '20000', '--', process.execPath, runner], { encoding: 'utf8' });
    assert.equal(r.status, 0, r.stderr);
    assert.match(r.stdout, new RegExp(`polluter: ${dir.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}/pollute\\.ts`));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

// A real Vitest, two files: leak.spec.ts leaves a value on globalThis, victim.spec.ts fails when it sees it. The default
// sequencer would not keep this order; the ordered sequencer and the flags find leak.spec.ts among decoys.
const VITEST_ROOTS = [fileURLToPath(new URL('../', import.meta.url)), fileURLToPath(new URL('../ui/', import.meta.url)), process.env.PADDOCK_VITEST_ROOT].filter(Boolean);
const vitestDir = VITEST_ROOTS.map((root) => {
  try {
    return join(createRequire(join(root, 'x.js')).resolve('vitest/package.json'), '..');
  } catch {
    return undefined;
  }
}).find(Boolean);

test('with a real Vitest: finds the spec that leaks globalThis state into the victim', { skip: vitestDir ? false : 'vitest is not installed (npm ci, or set PADDOCK_VITEST_ROOT to a folder whose node_modules has it)' }, () => {
  const dir = mkdtempSync(join(tmpdir(), 'polluter-vitest-'));
  try {
    mkdirSync(join(dir, 'node_modules'));
    const nm = join(vitestDir, '..');
    for (const name of ['vitest']) symlinkSync(join(nm, name), join(dir, 'node_modules', name), 'dir');
    writeFileSync(join(dir, 'package.json'), '{"type":"module","private":true}\n');
    const seq = fileURLToPath(new URL('./ordered-sequencer.mjs', import.meta.url));
    writeFileSync(join(dir, 'vitest.config.mjs'), `import { orderedSequence } from '${seq}';\nexport default { test: { isolate: false, include: ['**/*.spec.mjs'], sequence: { ...orderedSequence() } } };\n`);
    const spec = (name, body) => writeFileSync(join(dir, name), `import { test, expect } from 'vitest';\n${body}\n`);
    spec('a-decoy.spec.mjs', "test('decoy a', () => expect(1).toBe(1));");
    spec('b-leak.spec.mjs', "test('leaks', () => { globalThis.__leaked = 'x'; });");
    spec('c-decoy.spec.mjs', "test('decoy c', () => expect(1).toBe(1));");
    spec('d-victim.spec.mjs', "test('victim', () => expect(globalThis.__leaked).toBeUndefined());");
    const list = join(dir, 'before.txt');
    writeFileSync(list, ['a-decoy', 'b-leak', 'c-decoy'].map((n) => `${n}.spec.mjs`).join('\n'));
    const r = spawnSync(process.execPath, [SCRIPT, '--victim', 'd-victim.spec.mjs', '--before', list, '--cwd', dir, '--timeout', '120000', '--', process.execPath, join(vitestDir, 'vitest.mjs'), 'run', '--reporter=dot'], { encoding: 'utf8' });
    assert.equal(r.status, 0, r.stdout + r.stderr);
    assert.match(r.stdout, /polluter: \S*b-leak\.spec\.mjs/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
