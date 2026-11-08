import assert from 'node:assert/strict';
import { test } from 'node:test';
import { ownWireTypes, ratchets, windowSizingCalls } from '../src/checks/C78-dialog-sizing.mjs';
import { findingsOf, makeApp, messages } from './helpers.mjs';

test('windowSizingCalls finds production calls only', () => {
  assert.deepEqual(windowSizingCalls('fn a(w: &Window) { w.set_size(s); w.set_position(p); }').map((c) => c.name), ['set_size', 'set_position']);
  assert.deepEqual(windowSizingCalls('// w.set_size(s)\nfn a() {}\n'), []);
  assert.deepEqual(windowSizingCalls('#[cfg(test)]\nmod t { fn x(w: &Window) { w.set_min_size(m); } }\n'), []);
});

test('ratchets finds heights that follow the window', () => {
  assert.deepEqual(ratchets('a { height: 100vh; }\nb { min-height: 100%; }\nc { flex: 1; }\nd { flex-grow: 2; }\ne { height: 120px; }').map((r) => r.what), ['a viewport-height unit', 'a height of 100%', 'flex: 1', 'flex-grow']);
  assert.deepEqual(ratchets('a { margin: 0 }'), []);
});

test('C78 good: an app that leaves dialog sizing to paddock-dialogs', async () => {
  const app = makeApp({ 'app/src-tauri/src/lib.rs': 'use paddock_dialogs::tauri;\nfn a() {}\n', 'app/src/app/dialogs/about.ts': "@Component({ styles: 'p { margin: 0 }' }) export class A {}" });
  assert.deepEqual(await findingsOf('C78', app), []);
});

test('C78 bad: an app sizing a dialog window, calling the commands, and a ratchet in a form', async () => {
  const app = makeApp({
    'app/src-tauri/src/lib.rs': 'use paddock_dialogs::tauri;\nfn a(w: &Window) { w.set_size(s); }\n',
    'app/src/app/core/x.ts': "invoke('dialog_measure', {});",
    'app/src/app/dialogs/form.ts': "@Component({ styles: '.a { height: 100vh }' }) export class F {}",
    'app/src/app/dialogs/big-tool.ts': "@Component({ styles: '.a { height: 100vh }' }) export class T {}",
  });
  const f = await findingsOf('C78', app);
  assert.equal(f.length, 3);
  assert.match(messages(f), /set_size/);
  assert.match(messages(f), /dialog_measure is called outside/);
  assert.match(messages(f), /viewport-height unit/);
});

test('ownWireTypes finds hand-declared wire types and accepts aliases of the generated ones', () => {
  assert.deepEqual(ownWireTypes('export interface DialogMeasure {\n  a: 1;\n}\nexport type DialogStart = { id: string };').map((w) => w.name), ['DialogMeasure', 'DialogStart']);
  assert.deepEqual(ownWireTypes('export type DialogMeasure = DialogMeasureWire;\nexport interface DialogIo {}\nimport type { DialogInfo } from "x";'), []);
});

test('C78 bad: io.ts declaring a wire type of its own', async () => {
  const app = makeApp({ 'ui/projects/paddock-ui/shell/dialogs/src/io.ts': 'export interface DialogMeasure {\n  content: number;\n}\n' });
  assert.match(messages(await findingsOf('C78', app)), /io.ts declares the wire type DialogMeasure/);
});
