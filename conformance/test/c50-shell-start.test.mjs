import assert from 'node:assert/strict';
import { test } from 'node:test';
import { startProblems } from '../src/checks/C50-shell-start.mjs';
import { findingsOf, makeApp, messages } from './helpers.mjs';

const CARGO = '[dependencies]\npaddock-shell = { path = "../../../crates/paddock-shell" }\n';
const TEST = 'fn t() { let s = paddock_shell::testing::start(|| composed()); }\n';
const TEST_USE = 'use paddock_shell::testing::{Started, start};\nfn t() { start(|| composed()); }\n';
const GOOD_LIB = 'pub fn app() { shell.launch(launch).add_to(builder) }\n';
const OWN_START = 'fn run() { paddock_window::tauri::start(app, &launch)?; }\n';

test('startProblems: a start test and no own start pass; each gap is named', () => {
  assert.deepEqual(startProblems({ tests: [TEST], sources: [GOOD_LIB] }), []);
  assert.deepEqual(startProblems({ tests: [TEST_USE], sources: [GOOD_LIB] }), []);
  assert.match(startProblems({ tests: [], sources: [GOOD_LIB] })[0], /no start test/);
  assert.match(startProblems({ tests: [TEST], sources: [OWN_START] })[0], /opens its own main window/);
  assert.equal(startProblems({ tests: ['// paddock_shell::testing::start'], sources: [] }).length, 1, 'a comment is no test');
});

test('C50 bad: an app on the shell without a start test that starts its own window; good once fixed; others skipped', async () => {
  const bad = await findingsOf(
    'C50',
    makeApp({
      'apps/a/app/src-tauri/Cargo.toml': CARGO,
      'apps/a/app/src-tauri/src/lib.rs': OWN_START,
      'apps/a/app/src-tauri/tests/shell.rs': 'fn other() {}\n',
      'apps/b/app/src-tauri/Cargo.toml': '[dependencies]\ntauri = "2"\n',
      'apps/b/app/src-tauri/src/lib.rs': OWN_START,
    }),
  );
  assert.equal(bad.length, 2, messages(bad));
  assert.ok(bad.every((x) => JSON.stringify(x).includes('apps/a/')), messages(bad));
  const good = await findingsOf(
    'C50',
    makeApp({
      'apps/a/app/src-tauri/Cargo.toml': CARGO,
      'apps/a/app/src-tauri/src/lib.rs': GOOD_LIB,
      'apps/a/app/src-tauri/tests/shell.rs': TEST,
    }),
  );
  assert.deepEqual(good, []);
});
