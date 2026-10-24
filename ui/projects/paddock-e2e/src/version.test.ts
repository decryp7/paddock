import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { readVersion, RELEASE_VERSION } from './version.ts';

test('reads the version of a package.json or a Cargo.toml and checks its shape', () => {
  const dir = mkdtempSync(join(tmpdir(), 'paddock-e2e-'));
  try {
    const write = (name: string, text: string) => {
      const p = join(dir, name);
      writeFileSync(p, text);
      return p;
    };
    assert.equal(readVersion(write('package.json', '{"name":"x","version":"1.2.3"}')), '1.2.3');
    assert.equal(readVersion(write('Cargo.toml', '[package]\nname = "x"\nversion = "0.4.0-rc.1"\n\n[dependencies]\nversion = "9"\n')), '0.4.0-rc.1');
    assert.throws(() => readVersion(write('bad.json', '{"version":"latest"}')), /no release-number version \(found "latest"\)/);
    assert.throws(() => readVersion(write('none.json', '{}')), /found undefined/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
  assert.ok(RELEASE_VERSION.test('10.0.1+build.5'));
  assert.ok(!RELEASE_VERSION.test('1.2'));
});
