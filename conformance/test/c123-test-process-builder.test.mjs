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
