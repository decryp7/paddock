import assert from 'node:assert/strict';
import { test } from 'node:test';
import C125, { stepSites } from '../src/checks/C125-test-steps.mjs';

const sites = (text, file = 'crates/a/tests/it/x.rs') => stepSites(file.endsWith('.rs') ? `${text}\nuse std::process::Command;\n` : text, file).map((s) => `${s.rule}${s.line}`);

test('C125 rule A: a loop that runs a program to completion on every turn (Rust and Node)', () => {
  // The shape of the kept-clone tests before e0fdd0286: every phase, a job of each implementation, one after the other.
  assert.deepEqual(sites('#[test]\nfn t() {\n    for id in IDS {\n        let out = cmd(id).output().unwrap();\n    }\n}\n'), ['A4']);
  assert.deepEqual(sites('fn t() {\n    while more() {\n        c.status().unwrap();\n    }\n}\n'), ['A3']);
  assert.deepEqual(sites("for (const s of steps) {\n  spawnSync('bash', [s]);\n}\n", 'scripts/x.test.mjs'), ['A2']);
  assert.deepEqual(sites("for (const s of steps) execFileSync('git', s);\n", 'scripts/x.test.mjs'), ['A1'], 'a loop without a block runs its one statement');
});

test('C125 rule A: one run, a loop without a program, comments, strings and waivers are fine', () => {
  assert.deepEqual(sites('fn t() {\n    let out = c.output().unwrap();\n    for l in out.lines() {\n        check(l);\n    }\n}\n'), []);
  assert.deepEqual(sites('fn t() {\n    // for x in xs { c.output() }\n    let s = "for x in xs { c.output() }";\n}\n'), []);
  assert.deepEqual(sites('fn t() {\n    for a in ["init", "add"] {\n        // conformance: test-steps — two git commands that make the fixture, well under a second\n        git(a).output().unwrap();\n    }\n}\n'), []);
  assert.deepEqual(sites('fn t() {\n    // conformance: test-steps — two git commands that make the fixture, well under a second\n    for a in ["init", "add"] {\n        let out = git(a)\n            .arg("-q")\n            .output()\n            .unwrap();\n    }\n}\n'), [], 'a waiver on the loop covers its body');
});

test('C125 rule B: a fixed poll inside a loop (Node), in the header or the body', () => {
  // endHolders before ADR 0103: 300 looks 50 ms apart.
  assert.deepEqual(sites('for (let i = 0; i < 300 && alive(); i++) await new Promise((r) => setTimeout(r, 50));\n', 'scripts/vm-test-support.mjs'), ['B1']);
  assert.deepEqual(sites('for (let i = 0; i < 300 && alive(); i++) {\n  await new Promise((r) => setTimeout(r, 50));\n}\n', 'scripts/vm-test-support.mjs'), ['B2']);
  assert.deepEqual(sites('while (!done()) {\n  await new Promise((r) => setTimeout(r, 200));\n}\n', 'conformance/test/a.test.mjs'), ['B2']);
  assert.deepEqual(sites('setTimeout(() => ac.abort(), 5000);\n', 'conformance/test/a.test.mjs'), [], 'a single timer is not a poll');
});

test('C125 rule C: a spawn nothing ends (Rust) and a detached Node child nothing reaps', () => {
  assert.deepEqual(sites('fn t() {\n    let c = cmd.spawn().unwrap();\n    check();\n}\n'), ['C2']);
  assert.deepEqual(sites('fn t() {\n    let mut c = cmd.spawn().unwrap();\n    c.wait().unwrap();\n}\n'), []);
  assert.deepEqual(sites('fn t() {\n    let r = Reaper::new("t");\n    let c = r.spawn(&mut cmd).unwrap();\n}\n'), []);
  assert.deepEqual(sites('fn t() {\n    let c = paddock_proc::spawn_detached(&mut cmd).unwrap();\n    c.wait().unwrap();\n}\n'), ['C2']);
  assert.deepEqual(sites("const c = spawn('sh', [], { detached: true });\n", 'scripts/x.test.mjs'), ['C1']);
  assert.deepEqual(sites("import { Reaper } from './test-kit.mjs';\nconst c = spawn('sh', [], { detached: true });\n", 'scripts/x.test.mjs'), []);
});

test('C125 rules A and C read Rust files that build a Command only (an actor\'s spawn, a barrier\'s wait, an HTTP status are not programs)', () => {
  const plain = (text) => stepSites(text, 'crates/a/tests/it/x.rs').map((s) => `${s.rule}${s.line}`);
  assert.deepEqual(plain('fn t() {\n    let h = r.link.spawn().unwrap();\n    for x in xs {\n        x.status();\n        barrier.wait();\n    }\n}\n'), []);
});

test('C125 reads test code only and skips the kits', () => {
  const loop = 'fn t() {\n    for x in xs {\n        c.output().unwrap();\n    }\n}\n';
  assert.deepEqual(sites(loop, 'crates/a/src/lib.rs'), [], 'product code outside a test module');
  assert.deepEqual(sites(`fn p() {}\n#[cfg(test)]\nmod tests {\n${loop}}\n`, 'crates/a/src/lib.rs'), ['A6']);
  assert.deepEqual(sites(loop, 'crates/paddock-testkit-start/src/reap.rs'), []);
  assert.deepEqual(sites("for (;;) {\n  spawnSync('x');\n}\n", 'scripts/test-kit.mjs'), []);
  assert.deepEqual(sites("for (;;) {\n  spawnSync('x');\n}\n", 'scripts/affected.mjs'), [], 'a script, not a test');
});

test('C125 is report-only: its findings are warnings naming the rule, the ADR and the fix', () => {
  const files = { 'crates/a/tests/it/x.rs': 'use std::process::Command;\nfn t() {\n    for x in xs {\n        c.output().unwrap();\n    }\n}\n' };
  const ctx = { files: () => Object.keys(files), read: (f) => files[f] };
  const [f, ...rest] = C125.run(ctx);
  assert.equal(rest.length, 0);
  assert.equal(f.severity, 'warn');
  assert.equal(C125.mode, 'report-only');
  assert.match(f.message, /rule A, ADR 0103/);
  assert.match(f.fix, /run_cases/);
});
