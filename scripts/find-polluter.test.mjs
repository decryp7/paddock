import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { findPolluter, parseArgs } from './find-polluter.mjs';

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
    victim: 'v', before: 'b', cwd: '/x', cmd: ['npx', 'vitest'],
  });
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
    assert.match(r.stdout, /polluter: pollute\.spec\.ts/);
    const bad = spawnSync(process.execPath, [SCRIPT], { encoding: 'utf8' });
    assert.equal(bad.status, 2);
    assert.match(bad.stderr, /::error::usage/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
