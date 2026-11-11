// Doc facts checked from code (the 2026-11-06 redesign): the UI component diagrams name every feature folder, and every
// route from the Angular UI to Tauri, in their nodes' technology lines, so a new screen or a new route to the Rust shell
// cannot be left out of the drawing.
// Run: node --test docs/diagrams/src/*.test.mjs (from apps/pitwall; part of the app's checks in paddock.ci.json).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { PANELS } from './c43.mjs';

const ui = join(new URL('.', import.meta.url).pathname, '../../../app/src');
const appDir = join(ui, 'app');

/** The paths a panel names: each node's technology line split at " · ". */
const named = (nodes) => new Set(nodes.flatMap((n) => (n.tech ?? '').split(' · ').map((t) => t.trim())));

/** A source file's path as the diagrams name it: relative to src/app (or src), without `.ts`. */
const asNamed = (file) => relative(file.startsWith(appDir) ? appDir : ui, file).replace(/\.ts$/, '');

function walk(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(join(dir, e.name)) : [join(dir, e.name)]));
}
const sources = walk(ui)
  .filter((f) => f.endsWith('.ts') && !f.endsWith('.spec.ts'))
  .map((f) => ({ path: asNamed(f), file: f, text: readFileSync(f, 'utf8') }));

/** Files that call Tauri themselves. */
const direct = sources.filter((s) => /@tauri-apps\/|__TAURI_INTERNALS__/.test(s.text)).map((s) => s.path);
/** The Tauri helpers other files reach Tauri through. */
const HELPERS = ['shell/ipc', 'core/window-control'];
/** What a file imports (static and dynamic, relative specifiers only), as named paths. */
const importsOf = (s) =>
  [...s.text.matchAll(/(?:from\s+|import\s*\(\s*)'(\.{1,2}\/[^']+)'/g)].map((m) => asNamed(resolve(dirname(s.file), m[1]) + '.ts'));
/** Files one import away from a helper. */
const viaHelpers = sources.filter((s) => importsOf(s).some((p) => HELPERS.includes(p))).map((s) => s.path);
/**
 * Window bootstraps: they hand the helpers to the shell's frame (the page entry main.ts, the main, Settings, dialog and tray
 * windows, and Report a problem's providers). They are wiring, not routes of their own, so the state diagram does not draw them.
 */
const BOOTSTRAPS = [/^main$/, /^shell\/main-window$/, /^shell\/settings-window$/, /^shell\/dialog-[\w-]+$/, /^shell\/tray-window$/, /^shell\/diagnostics\.providers$/];

/** Routes to Tauri that the state panel's nodes leave unnamed. */
function missingRoutes(nodes) {
  const names = named(nodes);
  const routes = [...new Set([...direct, ...viaHelpers])].filter((p) => !BOOTSTRAPS.some((b) => b.test(p)));
  return routes.filter((p) => !names.has(p)).sort();
}

/** Feature folders that the features panel's nodes leave unnamed. */
function missingFeatures(nodes) {
  const names = named(nodes);
  const folders = readdirSync(join(appDir, 'features'), { withFileTypes: true }).filter((e) => e.isDirectory()).map((e) => e.name);
  return folders.filter((f) => !names.has(`features/${f}`));
}

const features = PANELS['c4-3-ui-features'];
const state = PANELS['c4-3-ui-state'];

test('the probes see the code and the panels', () => {
  assert.ok(features?.length && state?.length, 'c43.mjs records no c4-3-ui-features or c4-3-ui-state nodes');
  for (const p of ['core/tauri-backend', 'shell/ipc']) assert.ok(direct.includes(p), `${p} no longer reaches Tauri directly: fix the probe`);
  assert.ok(viaHelpers.includes('features/playground/chat-backend'), 'the import probe does not see chat-backend importing shell/ipc');
});

test('every feature folder is named on a c4-3-ui-features tech line', () => {
  assert.deepEqual(missingFeatures(features), [], 'name these as features/<folder> on a node of c4-3-ui-features');
});

test('every route to Tauri is named on a c4-3-ui-state tech line', () => {
  assert.deepEqual(missingRoutes(state), [], 'name these on a node of c4-3-ui-state, or add a window bootstrap to BOOTSTRAPS');
});

test('the checks fail when a node is dropped (mutation)', () => {
  assert.deepEqual(missingRoutes(state.filter((n) => n.id !== 'chat')), ['features/playground/chat-backend']);
  assert.ok(missingRoutes(state.filter((n) => n.id !== 'wc')).includes('core/window-control'));
  assert.deepEqual(missingFeatures(features.filter((n) => n.id !== 'dash')), ['dashboard']);
});
