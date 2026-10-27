// Run: node --test scripts/libduckdb.test.mjs
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { assetFor, parsePins } from './libduckdb.mjs';

const script = readFileSync(join(import.meta.dirname, '..', '..', 'crates', 'debrief-store-duck', 'scripts', 'fetch-libduckdb.sh'), 'utf8');

test('the pins come from fetch-libduckdb.sh, the one list CI and the docs use too', () => {
  const pins = parsePins(script);
  assert.match(pins.version, /^\d+\.\d+\.\d+$/);
  assert.deepEqual(
    pins.assets.map((a) => a.asset).sort(),
    [
      'duckdb-shared-libs-linux-amd64.tar.gz',
      'duckdb-shared-libs-linux-arm64.tar.gz',
      'duckdb-shared-libs-osx-arm64.tar.gz',
      'duckdb-shared-libs-windows-amd64.tar.gz',
      'duckdb-shared-libs-windows-arm64.tar.gz',
    ],
  );
  assert.ok(pins.assets.every((a) => /^[0-9a-f]{64}$/.test(a.sha)));
});

test('each platform gets its own asset; an unpinned one is an error that names the pinned ones', () => {
  const pins = parsePins(script);
  assert.equal(assetFor(pins, 'win32', 'x64').asset, 'duckdb-shared-libs-windows-amd64.tar.gz');
  assert.equal(assetFor(pins, 'win32', 'arm64').asset, 'duckdb-shared-libs-windows-arm64.tar.gz');
  assert.equal(assetFor(pins, 'linux', 'x64').asset, 'duckdb-shared-libs-linux-amd64.tar.gz');
  assert.equal(assetFor(pins, 'darwin', 'arm64').asset, 'duckdb-shared-libs-osx-arm64.tar.gz');
  // Intel Macs are not supported (DuckDB 2.1 drops them): no asset, and the error names what is pinned.
  assert.throws(() => assetFor(pins, 'darwin', 'x64'), /no pinned libduckdb for darwin-x64 \(pinned: duckdb-shared-libs-/);
  assert.throws(() => assetFor(pins, 'freebsd', 'x64'), /no pinned libduckdb for freebsd-x64 \(pinned: duckdb-shared-libs-/);
  assert.throws(() => parsePins('nothing here'), /no VERSION/);
});
