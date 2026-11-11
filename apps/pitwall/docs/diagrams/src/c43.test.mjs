// Doc facts checked from code (the 2026-11-06 redesign): the UI component diagrams in c43.mjs name every feature folder
// and every file that reaches Tauri, so a new screen or a new route to the Rust shell cannot be left out of the drawing.
// Run: node --test docs/diagrams/src/*.test.mjs (from apps/pitwall; part of the app's checks in paddock.ci.json).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { join, relative } from 'node:path';

const here = new URL('.', import.meta.url).pathname;
const source = readFileSync(join(here, 'c43.mjs'), 'utf8');
const ui = join(here, '../../../app/src');
const appDir = join(ui, 'app');

/** True when c43.mjs names `path` as a whole path (not as the start of a longer name: `core/backend` ≠ `core/backend-factory`). */
const names = (path) => new RegExp(`${path.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}(?![\\w-])`).test(source);

function walk(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(join(dir, e.name)) : [join(dir, e.name)]));
}

test('every feature folder is named in c43.mjs', () => {
  const folders = readdirSync(join(appDir, 'features'), { withFileTypes: true }).filter((e) => e.isDirectory()).map((e) => e.name);
  assert.ok(folders.length > 0, `no feature folders found under ${appDir}/features`);
  const missing = folders.filter((f) => !names(`features/${f}`));
  assert.deepEqual(missing, [], `name these in c43.mjs (c4-3-ui-features) as features/<folder>: ${missing.join(', ')}`);
});

test('every source file that reaches Tauri is named in c43.mjs', () => {
  const tauri = walk(ui)
    .filter((f) => f.endsWith('.ts') && !f.endsWith('.spec.ts'))
    .filter((f) => /@tauri-apps\/|__TAURI_INTERNALS__/.test(readFileSync(f, 'utf8')));
  assert.ok(tauri.length > 0, `no Tauri users found under ${ui}: the probe sees nothing`);
  const missing = tauri.map((f) => relative(f.startsWith(appDir) ? appDir : ui, f).replace(/\.ts$/, '')).filter((p) => !names(p));
  assert.deepEqual(missing, [], `name these in c43.mjs (c4-3-ui-state): ${missing.join(', ')}`);
});
