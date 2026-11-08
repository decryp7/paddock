import assert from 'node:assert/strict';
import { test } from 'node:test';
import { parseArgs } from './run.mjs';
import { MAC_ONLY_BASE, onlyRequired, parseOnly, rowWanted } from './only.mjs';

test('parseOnly splits, trims and dedupes known ids', () => {
  assert.deepEqual(parseOnly('mac-reopen, mac-reopen,hung-page'), ['mac-reopen', 'hung-page']);
});

test('parseOnly rejects an empty list and an unknown id', () => {
  assert.throws(() => parseOnly(''), /needs row ids/);
  assert.throws(() => parseOnly('nope'), /unknown row/);
});

test('onlyRequired is the base steps plus the picked rows', () => {
  const ids = onlyRequired(['mac-reopen']).map((r) => r.id);
  assert.deepEqual(ids.sort(), [...MAC_ONLY_BASE, 'mac-reopen'].sort());
});

test('rowWanted: everything without --only, only the picked rows with it', () => {
  assert.equal(rowWanted(undefined, 'hung-page'), true);
  assert.equal(rowWanted(['mac-reopen'], 'hung-page'), false);
  assert.equal(rowWanted(['mac-reopen'], 'mac-reopen'), true);
});

test('parseArgs --only needs the power suite and marks the run partial', () => {
  const base = ['--app', 'dyno', '--installer', '/x.dmg'];
  const o = parseArgs([...base, '--suite', 'power', '--only', 'mac-reopen']);
  assert.deepEqual(o.only, ['mac-reopen']);
  assert.equal(o.partial, true);
  assert.throws(() => parseArgs([...base, '--only', 'mac-reopen']), /needs --suite power/);
  assert.throws(() => parseArgs([...base, '--suite', 'power', '--only', 'x']), /unknown row/);
});

test('--only takes idle-action, whose required row is in the power manifest', () => {
  assert.deepEqual(parseOnly('idle-action'), ['idle-action']);
  assert.ok(onlyRequired(['idle-action']).some((r) => r.id === 'idle-action'));
});

test('parseArgs --audio-probe-pkg needs its sha256 and the power suite', () => {
  const base = ['--app', 'dyno', '--installer', '/x.dmg', '--suite', 'power'];
  const sha = 'a'.repeat(64);
  const o = parseArgs([...base, '--audio-probe-pkg', '/b.pkg', '--audio-probe-sha256', sha]);
  assert.equal(o.audioProbePkg, '/b.pkg');
  assert.equal(o.audioProbeSha256, sha);
  assert.throws(() => parseArgs([...base, '--audio-probe-pkg', '/b.pkg']), /--audio-probe-sha256/);
  assert.throws(() => parseArgs([...base, '--audio-probe-pkg', '/b.pkg', '--audio-probe-sha256', 'zz']), /64 hex/);
  assert.throws(() => parseArgs(['--app', 'dyno', '--installer', '/x.dmg', '--audio-probe-pkg', '/b.pkg', '--audio-probe-sha256', sha]), /needs --suite power/);
});
