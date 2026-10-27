import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { checkTag, tagVersion } from '../ci/check-tag.mjs';

const script = join(dirname(fileURLToPath(import.meta.url)), '..', 'ci', 'check-tag.mjs');

test('a tag names its version after the app prefix; anything else is refused', () => {
  assert.equal(tagVersion('pitcrew-v1.2.3', 'pitcrew-'), '1.2.3');
  assert.equal(tagVersion('v1.2.3-rc.1'), '1.2.3-rc.1');
  for (const bad of ['pitcrew-v1.2', 'other-v1.2.3', 'pitcrew-1.2.3', 'pitcrew-latest']) assert.throws(() => tagVersion(bad, 'pitcrew-'), /is not pitcrew-vX\.Y\.Z/, bad);
  assert.throws(() => tagVersion('pitcrewXv1.2.3', 'pitcrew.'), /is not/, 'the prefix is literal, not a pattern');
});

test('the tag and tauri.conf.json agree or the release stops', () => {
  assert.equal(checkTag('pitcrew-v1.0.3', 'pitcrew-', { version: '1.0.3' }), '1.0.3');
  assert.throws(() => checkTag('pitcrew-v1.0.3', 'pitcrew-', { version: '1.0.2' }), /says 1\.0\.3, tauri\.conf\.json says 1\.0\.2/);
});

test('the CLI exits 0 on a match and 1 on a mismatch', () => {
  const dir = mkdtempSync(join(tmpdir(), 'check-tag-'));
  try {
    const conf = join(dir, 'tauri.conf.json');
    writeFileSync(conf, JSON.stringify({ version: '1.0.3' }));
    assert.equal(spawnSync('node', [script, '--tag', 'pitcrew-v1.0.3', '--prefix', 'pitcrew-', '--conf', conf]).status, 0);
    const bad = spawnSync('node', [script, '--tag', 'pitcrew-v1.0.4', '--prefix', 'pitcrew-', '--conf', conf], { encoding: 'utf8' });
    assert.equal(bad.status, 1);
    assert.match(bad.stdout + bad.stderr, /::error::check-tag/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
