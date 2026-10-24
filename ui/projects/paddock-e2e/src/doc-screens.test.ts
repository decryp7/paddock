import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { planShots, recordHashes, sha256 } from './doc-screens.ts';

const screens = [{ name: 'home', url: '/#/' }, { name: 'tray', url: '/#/tray', themes: ['dark'] as const }];

test('planShots: screens x themes in table order, by theme or flat', () => {
  assert.deepEqual(planShots(screens).map((s) => s.file), ['dark/home.png', 'light/home.png', 'dark/tray.png']);
  assert.deepEqual(planShots(screens, 'flat').map((s) => s.file), ['home-dark.png', 'home-light.png', 'tray-dark.png']);
});

test('planShots refuses a duplicate or unsafe name', () => {
  assert.throws(() => planShots([screens[0]!, screens[0]!]), /listed twice/);
  for (const name of ['', 'Home', '../x', 'a/b', 'a b', '-a']) assert.throws(() => planShots([{ name, url: '/' }]), /lower-case words/, name);
});

test('sha256 is the hex digest of the bytes', () => {
  assert.equal(sha256(new Uint8Array()), 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855');
});

test('recordHashes merges, sorts, survives a damaged manifest and leaves no temporary file', () => {
  const dir = mkdtempSync(join(tmpdir(), 'paddock-e2e-shots-'));
  try {
    const path = join(dir, 'docs', 'screenshots', 'hashes.json');
    recordHashes(path, { 'light/b.png': '2', 'dark/a.png': '1' });
    const merged = recordHashes(path, { 'light/b.png': '3', 'dark/c.png': '4' });
    assert.deepEqual(Object.keys(merged), ['dark/a.png', 'dark/c.png', 'light/b.png']);
    assert.deepEqual(JSON.parse(readFileSync(path, 'utf8')), { 'dark/a.png': '1', 'dark/c.png': '4', 'light/b.png': '3' });
    writeFileSync(path, '{ not json');
    assert.deepEqual(recordHashes(path, { 'x.png': '9' }), { 'x.png': '9' });
    writeFileSync(path, JSON.stringify({ keep: 'a', drop: 5, list: [] }));
    assert.deepEqual(recordHashes(path, {}), { keep: 'a' });
    writeFileSync(path, '[1]');
    assert.deepEqual(recordHashes(path, {}), {});
    assert.deepEqual(readdirSync(join(dir, 'docs', 'screenshots')), ['hashes.json']);
    mkdirSync(join(dir, 'x'));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
