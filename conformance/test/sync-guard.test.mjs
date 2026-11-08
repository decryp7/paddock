import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { main } from '../src/cli.mjs';
import { PADDOCK } from './helpers.mjs';

test('sync --write refuses the monorepo root and names the next step; an app folder and a dry run are not refused', async () => {
  const errs = [];
  const code = await main(['sync', '--app', 'paddock', '--root', PADDOCK, '--paddock', PADDOCK, '--write'], { log: () => {}, err: (m) => errs.push(m) });
  assert.equal(code, 2);
  assert.match(errs.join('\n'), /would overwrite the monorepo's own root files/);
  assert.ok(!existsSync(join(PADDOCK, 'about.toml')), 'nothing was written');
  // a dry run (no --write) goes on to the sync itself
  const dry = [];
  await main(['sync', '--app', 'paddock', '--root', PADDOCK, '--paddock', PADDOCK], { log: (m) => dry.push(m), err: () => {} });
  assert.ok(!dry.join('\n').includes('would overwrite'));
});
