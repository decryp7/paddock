import assert from 'node:assert/strict';
import { test } from 'node:test';
import { capabilityProblems } from '../src/checks/C48-dialog-capability.mjs';
import { findingsOf, makeApp, messages } from './helpers.mjs';

const cap = (permissions, key = 'windows', labels = ['d-*']) => JSON.stringify({ identifier: 'dialog', [key]: labels, permissions });
const FULL = ['core:event:default', 'paddock-shell:dialog'];
const shell = '[dependencies]\npaddock-shell = { path = "../../../crates/paddock-shell" }\n';
const workbench = `${shell}paddock-workbench = { path = "../../../crates/paddock-workbench" }\n`;
const probs = (permissions, opts = { workbench: false }, key) => capabilityProblems([{ file: 'c.json', text: cap(permissions, key) }], opts);

test('capabilityProblems names what is missing', () => {
  assert.deepEqual(probs(FULL), []);
  assert.deepEqual(probs(FULL, { workbench: false }, 'webviews'), []);
  assert.deepEqual(probs([{ identifier: 'core:event:default' }, 'paddock-shell:dialog']), []);
  assert.deepEqual(probs(['paddock-shell:dialog']).length, 1);
  assert.match(probs(['paddock-shell:dialog'])[0], /core:event:default/);
  assert.match(probs(FULL, { workbench: true })[0], /paddock-workbench:dialog/);
  assert.match(capabilityProblems([{ file: 'c.json', text: cap(FULL, 'windows', ['main']) }], { workbench: false })[0], /no capability names/);
  assert.match(capabilityProblems([{ file: 'c.json', text: '{' }], { workbench: false })[0], /not valid JSON/);
  assert.deepEqual(capabilityProblems([], { workbench: false }).length, 1);
});

test('capabilityProblems adds up the capabilities that name d-*', () => {
  const texts = [
    { file: 'a.json', text: cap(['core:event:default']) },
    { file: 'b.json', text: cap(['paddock-shell:dialog'], 'webviews') },
    { file: 'main.json', text: cap(['paddock-workbench:dialog'], 'windows', ['main']) },
  ];
  assert.deepEqual(capabilityProblems(texts, { workbench: false }), []);
  assert.equal(capabilityProblems(texts, { workbench: true }).length, 1);
});

test('C48 bad: no dialog capability at all, an incomplete one, a workbench app without its own set', async () => {
  const f = await findingsOf(
    'C48',
    makeApp({
      'apps/none/src-tauri/Cargo.toml': shell,
      'apps/none/src-tauri/capabilities/default.json': cap(['core:default'], 'windows', ['main']),
      'apps/nocaps/src-tauri/Cargo.toml': shell,
      'apps/a/src-tauri/Cargo.toml': shell,
      'apps/a/src-tauri/capabilities/dialog.json': cap(['core:event:default']),
      'apps/w/src-tauri/Cargo.toml': workbench,
      'apps/w/src-tauri/capabilities/dialog.json': cap(FULL),
    }),
  );
  assert.equal(f.length, 4);
  assert.deepEqual(f.map((x) => x.file).sort(), ['apps/a/src-tauri/Cargo.toml', 'apps/nocaps/src-tauri/Cargo.toml', 'apps/none/src-tauri/Cargo.toml', 'apps/w/src-tauri/Cargo.toml']);
  assert.match(messages(f), /no capability names the dialog windows/);
  assert.match(messages(f), /grants `paddock-shell:dialog`/);
  assert.match(messages(f), /grants `paddock-workbench:dialog`/);
});

test('C48 sees the shell dependency however Cargo.toml spells it', async () => {
  const spellings = {
    dotted: '[dependencies]\npaddock-shell.workspace = true\n',
    table: '[target.\'cfg(unix)\'.dependencies.paddock-shell]\npath = "../../../crates/paddock-shell"\n',
    renamed: '[dependencies]\nshell = { path = "../x", package = "paddock-shell" }\n',
  };
  for (const [name, toml] of Object.entries(spellings)) {
    const f = await findingsOf('C48', makeApp({ [`apps/${name}/src-tauri/Cargo.toml`]: toml }));
    assert.equal(f.length, 1, name);
  }
  const g = await findingsOf('C48', makeApp({ 'apps/x/src-tauri/Cargo.toml': '[package]\nname = "paddock-shell-demo"\n[dependencies]\ntauri = "2"\n' }));
  assert.deepEqual(g, []);
});

test('C48 good: complete grants (windows or webviews, split over files), and apps not on the shared shell', async () => {
  const f = await findingsOf(
    'C48',
    makeApp({
      'apps/a/src-tauri/Cargo.toml': shell,
      'apps/a/src-tauri/capabilities/dialog.json': cap(FULL),
      'apps/w/src-tauri/Cargo.toml': workbench,
      'apps/w/src-tauri/capabilities/ev.json': cap(['core:event:default'], 'webviews'),
      'apps/w/src-tauri/capabilities/dialog.json': cap(['paddock-shell:dialog', 'paddock-workbench:dialog']),
      'apps/own/app/src-tauri/Cargo.toml': '[dependencies]\ntauri = "2"\n',
    }),
  );
  assert.deepEqual(f, []);
});
