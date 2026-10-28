import assert from 'node:assert/strict';
import { test } from 'node:test';
import { hiddenConstructorCalls } from '../src/checks/C40-dialog-markers.mjs';
import { findingsOf, makeApp, messages } from './helpers.mjs';

const prod = 'pub static T: &[DialogCallable] = &[DialogCallable::__command("app_status"), DialogCallable::__prefix("settings_")];\n';

test('hiddenConstructorCalls finds production calls and ignores comments, strings, doctests and the test module', () => {
  assert.deepEqual(hiddenConstructorCalls(prod).map((c) => c.name), ['__command', '__prefix']);
  assert.equal(hiddenConstructorCalls(`\n\n${prod}`)[0].line, 3);
  assert.deepEqual(hiddenConstructorCalls('// DialogCallable::__command("x")\n/// ```\n/// DialogCallable::__prefix("x")\n/// ```\nfn a() { let s = "__command(x)"; }\n'), []);
  assert.deepEqual(hiddenConstructorCalls(`fn a() {}\n#[cfg(test)]\nmod tests {\n    fn t() { ${prod} }\n}\n`), []);
  assert.deepEqual(hiddenConstructorCalls(`fn a() { DialogCallable::__command ("x"); }\n`).map((c) => c.name), ['__command']);
});

test('C40 bad: a production call outside paddock-commands', async () => {
  const f = await findingsOf('C40', makeApp({ 'crates/paddock-dialogs/src/table.rs': prod, 'apps/x/src-tauri/src/dialogs.rs': prod }));
  assert.equal(f.length, 4);
  assert.equal(f[0].severity, 'error');
  assert.match(messages(f), /__command\(\.\.\) is called outside paddock-commands/);
  assert.deepEqual([...new Set(f.map((x) => x.file))].sort(), ['apps/x/src-tauri/src/dialogs.rs', 'crates/paddock-dialogs/src/table.rs']);
});

test('C40 good: paddock-commands itself, test modules, test files and doctests', async () => {
  const f = await findingsOf(
    'C40',
    makeApp({
      'crates/paddock-commands/src/dialog.rs': prod,
      'crates/paddock-dialogs/src/registry.rs': `pub fn a() {}\n#[cfg(test)]\nmod tests {\n${prod}}\n`,
      'crates/paddock-dialogs/src/tauri/tests.rs': prod,
      'crates/paddock-shell/tests/manifest.rs': prod,
      'crates/paddock-shell/src/lib.rs': '//! ```\n//! DialogCallable::__command("x");\n//! ```\n',
      'ui/a.ts': '__command("x")',
    }),
  );
  assert.deepEqual(f, []);
});
