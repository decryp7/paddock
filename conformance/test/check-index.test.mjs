import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { inlineElements } from '../ci/check-index.mjs';

const script = join(dirname(fileURLToPath(import.meta.url)), '..', 'ci', 'check-index.mjs');

test('inline styles and scripts are found', () => {
  const html = '<head><style>:root{}</style><link rel="stylesheet" href="s.css"></head><body><script>x()</script></body>';
  assert.deepEqual(inlineElements(html), ['<style>', '<script>']);
});

test('external scripts and stylesheets pass', () => {
  const html = '<link rel="stylesheet" href="s.css"><script src="main.js" type="module"></script>';
  assert.deepEqual(inlineElements(html), []);
});

test('the CLI fails on an inline element, passes a clean file, and still runs when reached through a link', () => {
  const dir = mkdtempSync(join(tmpdir(), 'check-index-'));
  try {
    const bad = join(dir, 'bad.html');
    const good = join(dir, 'good.html');
    writeFileSync(bad, '<style>:root{}</style>');
    writeFileSync(good, '<script src="main.js"></script>');
    assert.equal(spawnSync('node', [script, good]).status, 0);
    const failed = spawnSync('node', [script, bad], { encoding: 'utf8' });
    assert.equal(failed.status, 1);
    assert.match(failed.stderr, /inline elements that break the app's styles/);
    // stable-workspace links the checkout: the script must not exit 0 without checking (the old copy compared file:// URLs).
    const linked = join(dir, 'linked.mjs');
    symlinkSync(script, linked);
    assert.equal(spawnSync('node', [linked, bad]).status, 1);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
