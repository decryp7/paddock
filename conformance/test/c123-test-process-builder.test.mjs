import assert from 'node:assert/strict';
import { test } from 'node:test';
import { spawnSites } from '../src/checks/C123-test-process-builder.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const lines = (text, path = 'crates/a/tests/it/x.rs') => spawnSites(text, path).map((s) => s.line);

test('C123 bad: a plain Command for the dev binary, cargo or node in a test file, also split over lines', () => {
  assert.deepEqual(lines('fn t() {\n    Command::new("node").output();\n    paddock_proc::command(\n        env!("CARGO"),\n    );\n}\n'), [2, 3]);
  assert.deepEqual(lines('fn t() {\n    std::process::Command::new(env!("CARGO_BIN_EXE_paddock-dev"));\n    quiet_command(std::env::var("CARGO").unwrap());\n}\n'), [2, 3]);
});

test('C123 good: the builder, another program, a comment, and production code outside the test module', () => {
  assert.deepEqual(lines('fn t() {\n    TestCommand::new("node").output();\n    Command::new("git").output();\n    // Command::new("cargo")\n}\n'), []);
  assert.deepEqual(lines('fn run() {\n    Command::new("cargo");\n}\n#[cfg(test)]\nmod tests {\n    fn t() {\n        Command::new("cargo");\n    }\n}\n', 'crates/a/src/lib.rs'), [7]);
});

test('C123: the dev binary handed on is a site unless the function builds with TestCommand', () => {
  assert.deepEqual(lines('fn t() {\n    let bin = env!("CARGO_BIN_EXE_paddock-dev");\n    run(bin);\n}\n'), [2]);
  assert.deepEqual(lines('fn t() {\n    let bin = env!("CARGO_BIN_EXE_paddock-dev");\n    TestCommand::new("sh").arg(bin);\n}\n'), []);
});

test('C123: a waiver needs a reason; the bare comment is a finding that says so, and it is report-only (ER-41)', async () => {
  const t = (c) => `fn t() {\n    // ${c}\n    Command::new("cargo");\n}\n`;
  const root = makeApp({ 'a/tests/ok.rs': t('conformance: test-env — a probe of the shell'), 'a/tests/b.rs': t('conformance: test-env') });
  const f = await findingsOf('C123', root, { app: 'paddock' });
  assert.equal(f.length, 1);
  assert.equal(f[0].severity, 'warn');
  assert.equal(f[0].file, 'a/tests/b.rs');
  assert.match(f[0].message, /waiver comment has no reason/);
});

test('C123: no Rust test file is check broken, never a pass', async () => {
  const f = await findingsOf('C123', makeApp({ 'README.md': 'x' }), { app: 'paddock' });
  assert.equal(f.length, 1);
  assert.equal(f[0].severity, 'error');
  assert.equal(f[0].unwaivable, true);
});

test('C123: any CARGO_BIN_EXE binary, also through a name bound to it, and the machine PATH spliced into a TestCommand', () => {
  assert.deepEqual(lines('fn t() {\n    Command::new(env!("CARGO_BIN_EXE_heavy")).output();\n}\n'), [2]);
  assert.deepEqual(lines('pub const BIN: &str = env!("CARGO_BIN_EXE_heavy");\nfn t() {\n    let mut c = command(BIN);\n    Command::new(&BIN);\n}\n'), [3, 4]);
  // handed to the code under test, which starts it itself: not a test's Command
  assert.deepEqual(lines('fn t() {\n    let s = SpawnSpec::new(env!("CARGO_BIN_EXE_fake-child"), "x");\n}\n'), []);
  assert.deepEqual(lines('fn t() {\n    let p = std::env::var("PATH").unwrap();\n    TestCommand::new("x").env("PATH", p);\n}\n'), [2]);
  assert.deepEqual(lines('fn t() {\n    let p = std::env::var("PATH").unwrap();\n    check(p);\n}\n'), []);
});

// Review of a8fca0ebd (P2 3): the gaps, each a probe that failed before the fix.
test('C123: a function that returns a built binary binds its name, as a const does', () => {
  assert.deepEqual(lines('fn child_program() -> PathBuf {\n    PathBuf::from(env!("CARGO_BIN_EXE_x"))\n}\nfn t() {\n    Command::new(child_program());\n}\n'), [5]);
});

test('C123: rustc, rustup, npm, npx, .exe spellings and a path joined to dev are judged', () => {
  const t = 'fn t() {\n    Command::new("rustc");\n    Command::new("rustup");\n    Command::new("npm");\n    Command::new("npx");\n    Command::new("cargo.exe");\n    Command::new(env!("RUSTC"));\n    Command::new(std::env::var("RUSTC").unwrap());\n    Command::new(root.join("dev"));\n}\n';
  assert.deepEqual(lines(t), [2, 3, 4, 5, 6, 7, 8, 9]);
});

test('C123: TestCommand named only in a comment does not count for the function', () => {
  assert.deepEqual(lines('fn t() {\n    // TestCommand is not used here\n    let bin = env!("CARGO_BIN_EXE_paddock-dev"); // nor TestCommand\n    run(bin);\n}\n'), [3]);
});

test('C123: any cfg naming test opens the test part, cfg(not(test)) does not', async () => {
  const src = 'fn run() {\n    Command::new("cargo");\n}\n#[cfg(all(test, unix))]\nmod tests {\n    fn t() {\n        Command::new("cargo");\n    }\n}\n';
  assert.deepEqual(lines(src, 'crates/a/src/lib.rs'), [7]);
  assert.deepEqual(lines('#[cfg(not(test))]\nfn run() {\n    Command::new("cargo");\n}\n', 'crates/a/src/lib.rs'), []);
  const f = await findingsOf('C123', makeApp({ 'a/src/lib.rs': src, 'a/tests/x.rs': 'fn t() {}\n' }), { app: 'paddock' });
  assert.deepEqual(f.map((x) => `${x.file}:${x.line}`), ['a/src/lib.rs:7']);
});

test('C123: a pub function of a shared module binds its name in the other test files of its crate only', async () => {
  const common = 'pub fn child_program() -> PathBuf {\n    PathBuf::from(env!("CARGO_BIN_EXE_x"))\n}\n';
  const user = 'fn t() {\n    paddock_proc::command(child_program());\n}\n';
  const f = await findingsOf('C123', makeApp({ 'a/tests/it/common/mod.rs': common, 'a/tests/it/edges.rs': user, 'b/tests/it/other.rs': user }), { app: 'paddock' });
  assert.deepEqual(f.map((x) => `${x.file}:${x.line}`), ['a/tests/it/edges.rs:2']);
});
