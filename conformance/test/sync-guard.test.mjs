import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { main } from '../src/cli.mjs';

// Only ever a temp tree shaped like the monorepo root (ER-14): if the guard broke, `sync --write` would write there, not here.
test('sync --write refuses a monorepo root and names the next step; nothing is written', async () => {
  const root = mkdtempSync(join(tmpdir(), 'sync-guard-'));
  try {
    for (const d of ['apps', '.gitea', join('tools', 'paddock-dev')]) mkdirSync(join(root, d), { recursive: true });
    const before = readdirSync(root).sort();
    const errs = [];
    const code = await main(['sync', '--app', 'paddock', '--root', root, '--paddock', root, '--write'], { log: () => {}, err: (m) => errs.push(m) });
    assert.equal(code, 2);
    assert.match(errs.join('\n'), /would overwrite the monorepo's own root files/);
    assert.deepEqual(readdirSync(root).sort(), before, 'nothing was written');
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('a folder that is not a monorepo root is not refused by the guard', async () => {
  const root = mkdtempSync(join(tmpdir(), 'sync-guard-'));
  try {
    const errs = [];
    await main(['sync', '--app', 'x', '--root', root, '--paddock', root], { log: () => {}, err: (m) => errs.push(m) });
    assert.ok(!errs.join('\n').includes('would overwrite'));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
