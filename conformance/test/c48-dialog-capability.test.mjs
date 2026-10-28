import assert from 'node:assert/strict';
import { test } from 'node:test';
import { capabilityProblems } from '../src/checks/C48-dialog-capability.mjs';
import { findingsOf, makeApp, messages } from './helpers.mjs';

const cap = (windows, permissions) => JSON.stringify({ identifier: 'dialog', windows, permissions });
const shell = '[dependencies]\npaddock-shell = { path = "../../../crates/paddock-shell" }\n';
const workbench = `${shell}paddock-workbench = { path = "../../../crates/paddock-workbench" }\n`;

test('capabilityProblems names what is missing', () => {
  assert.deepEqual(capabilityProblems(cap(['d-*'], ['core:event:default', 'paddock-shell:dialog']), { workbench: false }), []);
  assert.equal(capabilityProblems(cap(['d-*'], ['paddock-shell:dialog']), { workbench: true }).length, 1);
  assert.equal(capabilityProblems(cap(['main'], []), { workbench: false }).length, 2);
  assert.match(capabilityProblems('{', { workbench: false })[0], /not valid JSON/);
});

test('C48 bad: a shell app whose dialog capability lacks the set, or a workbench app without its own', async () => {
  const f = await findingsOf(
    'C48',
    makeApp({
      'apps/a/src-tauri/Cargo.toml': shell,
      'apps/a/src-tauri/capabilities/dialog.json': cap(['d-*'], ['core:event:default']),
      'apps/w/src-tauri/Cargo.toml': workbench,
      'apps/w/src-tauri/capabilities/dialog.json': cap(['d-*'], ['paddock-shell:dialog']),
    }),
  );
  assert.equal(f.length, 2);
  assert.match(messages(f), /does not grant `paddock-shell:dialog`/);
  assert.match(messages(f), /does not grant `paddock-workbench:dialog`/);
});

test('C48 good: complete grants, and apps that are not on the shared shell', async () => {
  const f = await findingsOf(
    'C48',
    makeApp({
      'apps/a/src-tauri/Cargo.toml': shell,
      'apps/a/src-tauri/capabilities/dialog.json': cap(['d-*'], ['core:event:default', 'paddock-shell:dialog']),
      'apps/w/src-tauri/Cargo.toml': workbench,
      'apps/w/src-tauri/capabilities/dialog.json': cap(['d-*'], ['paddock-shell:dialog', 'paddock-workbench:dialog']),
      'apps/own/app/src-tauri/Cargo.toml': '[dependencies]\ntauri = "2"\n',
      'apps/own/app/src-tauri/capabilities/dialog.json': cap(['x'], []),
    }),
  );
  assert.deepEqual(f, []);
});
