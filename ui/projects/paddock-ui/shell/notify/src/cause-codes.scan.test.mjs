import assert from 'node:assert/strict';
import { existsSync, readdirSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import { READ_CONCURRENCY, readBounded, rustSources, scanEmitted, TABLE_ONLY } from './cause-codes-scan.ts';
import { KNOWN_CAUSE_CODES } from './service-status-text.ts';

// ER-21: every health cause code the Rust side emits has words in the chip's table, and every word in the table is for a code
// Rust emits. This is the scan over the real tree (~2k files): it runs under `node --test` (npm run test:scripts), which has no
// per-test wall limit, instead of in the unit slice, where a 5 s limit made it fail whenever the machine or the disk was loaded.
// The scan logic itself is judged on fixtures in cause-codes.spec.ts.

test('the Rust sources and the chip table agree', async () => {
  const root = ['.', '..', '../..', '../../..', '../../../..', '../../../../..', '../../../../../..'].map((p) => new URL(p, import.meta.url).pathname).find((p) => existsSync(`${p}/crates/Cargo.toml`));
  assert.ok(root, 'the repo root (a folder with crates/Cargo.toml) above this file');
  const files = [...rustSources({ readdirSync }, `${root}/crates`), ...rustSources({ readdirSync }, `${root}/apps`)];
  const texts = await readBounded(files, (f) => readFile(f, 'utf8'), READ_CONCURRENCY);
  const { emitted, unlisted } = scanEmitted(files, texts);
  assert.deepEqual(unlisted, [], 'a .code( call with a non-literal argument: list it in NON_LITERAL with the codes it can produce');
  assert.ok(emitted.size > 10);
  assert.deepEqual([...emitted].filter(([c]) => !KNOWN_CAUSE_CODES.includes(c)).map(([c, f]) => `${c} (${f})`), []);
  assert.deepEqual(KNOWN_CAUSE_CODES.filter((c) => !emitted.has(c) && !(c in TABLE_ONLY)), []);
});
