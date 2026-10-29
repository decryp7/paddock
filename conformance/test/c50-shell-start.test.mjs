import assert from 'node:assert/strict';
import { test } from 'node:test';
import { startProblems } from '../src/checks/C50-shell-start.mjs';
import { findingsOf, makeApp, messages } from './helpers.mjs';

const CARGO = '[dependencies]\npaddock-shell = { path = "../../../crates/paddock-shell" }\n';
const TEST = 'fn t() { let s = paddock_shell::testing::start(|| composed()); assert!(s.unwrap().main_window); }\n';
const TEST_USE = 'use paddock_shell::testing::{Started, start};\nfn t() { assert_eq!(start(|| composed()), Ok(Started { main_window: true, tray: true })); }\n';
const LOG = 'fn install_log() { paddock_log::Logging::new(config).install(); }\n';
const GOOD_LIB = `pub fn app() { shell.launch(launch).add_to(builder) }\n${LOG}`;
const ok = (over = {}) => startProblems({ tests: [TEST], sources: [GOOD_LIB], ...over });

test('startProblems: a start test, no own start, no own Tauri setup and a log pass', () => {
  assert.deepEqual(ok(), []);
  assert.deepEqual(ok({ tests: [TEST_USE] }), []);
});

test('startProblems: no start test, or one that never looks at the main window', () => {
  assert.match(ok({ tests: [] })[0], /no start test/);
  assert.equal(ok({ tests: ['// paddock_shell::testing::start(|| x); main_window'] }).length, 1, 'a comment is no test');
  assert.match(ok({ tests: ['fn t() { paddock_shell::testing::start(|| x).unwrap(); }'] })[0], /main_window/);
});

test('startProblems: the app opens its own window, also through a use import or create_main', () => {
  for (const src of [
    'fn run() { paddock_window::tauri::start(app, &launch)?; }',
    'use paddock_window::tauri::start;\nfn run() { start(app, &launch)?; }',
    'use paddock_window::tauri::{FixedPrefs, start, Windows};',
    'fn f() { paddock_window::tauri::create_main(app)?; }',
    'use paddock_window::tauri::{create_main as main_window};',
  ]) {
    const p = ok({ sources: [`${src}\n${LOG}`] });
    assert.equal(p.length, 1, src);
    assert.match(p[0], /opens its own main window/, src);
  }
  assert.deepEqual(ok({ sources: [`fn started() { let start = 1; }\n${LOG}`] }), [], 'another `start` is fine');
});

test('startProblems: a Tauri setup of its own overwrites the shell start', () => {
  const own = `fn run() {\n  let b = tauri::Builder::default()\n    .plugin(x)\n    .setup(|app| Ok(()));\n}\n${LOG}`;
  assert.match(ok({ sources: [own] })[0], /tauri::Builder.*setup/);
  const shellSetup = `fn shell() { Builder::new(id).setup(move |app| Ok(())) }\nfn run() { let b = tauri::Builder::default().plugin(x); }\n${LOG}`;
  assert.deepEqual(ok({ sources: [shellSetup] }), [], "the shell's setup is fine");
});

test('startProblems: the app installs paddock_log, itself or through the shell', () => {
  assert.match(ok({ sources: ['pub fn app() { shell.add_to(builder) }\n'] })[0], /paddock_log/);
  assert.deepEqual(ok({ sources: ['fn run() { let _log = paddock_shell::logging::install(&identity); }\n'] }), []);
});

test('C50 bad: an app on the shell with the gaps; good once fixed; others skipped', async () => {
  const bad = await findingsOf(
    'C50',
    makeApp({
      'apps/a/app/src-tauri/Cargo.toml': CARGO,
      'apps/a/app/src-tauri/src/lib.rs': 'fn run() { paddock_window::tauri::start(app, &launch)?; }\n',
      'apps/a/app/src-tauri/tests/shell.rs': 'fn other() {}\n',
      'apps/b/app/src-tauri/Cargo.toml': '[dependencies]\ntauri = "2"\n',
      'apps/b/app/src-tauri/src/lib.rs': 'fn run() { paddock_window::tauri::start(app, &launch)?; }\n',
    }),
  );
  assert.equal(bad.length, 3, messages(bad));
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
