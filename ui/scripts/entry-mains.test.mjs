import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { addEntryMains } from './entry-mains.mjs';

test('every secondary entry folder gets a main equal to its module, once', () => {
  const dist = mkdtempSync(join(tmpdir(), 'entry-mains-'));
  mkdirSync(join(dist, 'format'));
  mkdirSync(join(dist, 'shell/about'), { recursive: true });
  const entry = (m) => JSON.stringify({ module: m, typings: 't.d.ts' });
  writeFileSync(join(dist, 'format/package.json'), entry('../fesm2022/a.mjs'));
  writeFileSync(join(dist, 'shell/about/package.json'), entry('../../fesm2022/b.mjs'));
  assert.equal(addEntryMains(dist).length, 2);
  assert.equal(JSON.parse(readFileSync(join(dist, 'format/package.json'), 'utf8')).main, '../fesm2022/a.mjs');
  assert.equal(JSON.parse(readFileSync(join(dist, 'shell/about/package.json'), 'utf8')).main, '../../fesm2022/b.mjs');
  assert.equal(addEntryMains(dist).length, 0);
});
