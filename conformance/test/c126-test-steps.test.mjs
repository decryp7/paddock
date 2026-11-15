import assert from 'node:assert/strict';
import { test } from 'node:test';
import C126, { stepSites } from '../src/checks/C126-test-steps.mjs';

const sites = (text, file = 'crates/a/tests/it/x.rs') => stepSites(file.endsWith('.rs') ? `${text}\nuse std::process::Command;\n` : text, file).map((s) => `${s.rule}${s.line}`);

test('C126 rule A: a loop that runs a program to completion on every turn (Rust and Node)', () => {
  // The shape of the kept-clone tests before e0fdd0286: every phase, a job of each implementation, one after the other.
  assert.deepEqual(sites('#[test]\nfn t() {\n    for id in IDS {\n        let out = cmd(id).output().unwrap();\n    }\n}\n'), ['A4']);
  assert.deepEqual(sites('fn t() {\n    let mut c = Command::new("x");\n    while more() {\n        c.status().unwrap();\n    }\n}\n'), ['A4']);
  assert.deepEqual(sites("for (const s of steps) {\n  spawnSync('bash', [s]);\n}\n", 'scripts/x.test.mjs'), ['A2']);
  assert.deepEqual(sites("for (const s of steps) execFileSync('git', s);\n", 'scripts/x.test.mjs'), ['A1'], 'a loop without a block runs its one statement');
});

test('C126 rule A: one run, a loop without a program, comments, strings and waivers are fine', () => {
  assert.deepEqual(sites('fn t() {\n    let out = c.output().unwrap();\n    for l in out.lines() {\n        check(l);\n    }\n}\n'), []);
  assert.deepEqual(sites('fn t() {\n    // for x in xs { c.output() }\n    let s = "for x in xs { c.output() }";\n}\n'), []);
  assert.deepEqual(sites('fn t() {\n    for a in ["init", "add"] {\n        // conformance: test-steps — two git commands that make the fixture, well under a second\n        git(a).output().unwrap();\n    }\n}\n'), []);
  assert.deepEqual(sites('fn t() {\n    // conformance: test-steps — two git commands that make the fixture, well under a second\n    for a in ["init", "add"] {\n        let out = git(a)\n            .arg("-q")\n            .output()\n            .unwrap();\n    }\n}\n'), [], 'a waiver on the loop covers its body');
});

test('C126 rule B: a fixed poll inside a loop (Node), in the header or the body', () => {
  // endHolders before ADR 0103: 300 looks 50 ms apart.
  assert.deepEqual(sites('for (let i = 0; i < 300 && alive(); i++) await new Promise((r) => setTimeout(r, 50));\n', 'scripts/vm-test-support.mjs'), ['B1']);
  assert.deepEqual(sites('for (let i = 0; i < 300 && alive(); i++) {\n  await new Promise((r) => setTimeout(r, 50));\n}\n', 'scripts/vm-test-support.mjs'), ['B2']);
  assert.deepEqual(sites('while (!done()) {\n  await new Promise((r) => setTimeout(r, 200));\n}\n', 'conformance/test/a.test.mjs'), ['B2']);
  assert.deepEqual(sites('setTimeout(() => ac.abort(), 5000);\n', 'conformance/test/a.test.mjs'), [], 'a single timer is not a poll');
});

test('C126 rule C: a spawn nothing ends (Rust) and a detached Node child nothing reaps', () => {
  assert.deepEqual(sites('fn t() {\n    let c = cmd.spawn().unwrap();\n    check();\n}\n'), ['C2']);
  assert.deepEqual(sites('fn t() {\n    let mut c = cmd.spawn().unwrap();\n    c.wait().unwrap();\n}\n'), []);
  assert.deepEqual(sites('fn t() {\n    let r = Reaper::new("t");\n    let c = r.spawn(&mut cmd).unwrap();\n}\n'), []);
  assert.deepEqual(sites('fn t() {\n    let c = paddock_proc::spawn_detached(&mut cmd).unwrap();\n    c.wait().unwrap();\n}\n'), ['C2']);
  assert.deepEqual(sites("const c = spawn('sh', [], { detached: true });\n", 'scripts/x.test.mjs'), ['C1']);
  assert.deepEqual(sites("import { Reaper } from './test-kit.mjs';\nconst r = new Reaper('t');\nconst c = spawn('sh', [], { detached: true });\n", 'scripts/x.test.mjs'), []);
});

test('C126 rules A and C read Rust files that build a Command only (an actor\'s spawn, a barrier\'s wait, an HTTP status are not programs)', () => {
  const plain = (text) => stepSites(text, 'crates/a/tests/it/x.rs').map((s) => `${s.rule}${s.line}`);
  assert.deepEqual(plain('fn t() {\n    let h = r.link.spawn().unwrap();\n    for x in xs {\n        x.status();\n        barrier.wait();\n    }\n}\n'), []);
});

test('C126 reads test code only and skips the kits', () => {
  const loop = 'fn t() {\n    for x in xs {\n        c.output().unwrap();\n    }\n}\n';
  assert.deepEqual(sites(loop, 'crates/a/src/lib.rs'), [], 'product code outside a test module');
  assert.deepEqual(sites(`fn p() {}\n#[cfg(test)]\nmod tests {\n${loop}}\n`, 'crates/a/src/lib.rs'), ['A6']);
  assert.deepEqual(sites(loop, 'crates/paddock-testkit-start/src/reap.rs'), []);
  assert.deepEqual(sites("for (;;) {\n  spawnSync('x');\n}\n", 'scripts/test-kit.mjs'), []);
  assert.deepEqual(sites("for (;;) {\n  spawnSync('x');\n}\n", 'scripts/affected.mjs'), [], 'a script, not a test');
});

test('C126 is report-only: its findings are warnings naming the rule, the ADR and the fix', () => {
  const files = { 'crates/a/tests/it/x.rs': 'use std::process::Command;\nfn t() {\n    for x in xs {\n        c.output().unwrap();\n    }\n}\n' };
  const ctx = { files: () => Object.keys(files), read: (f) => files[f] };
  const [f, ...rest] = C126.run(ctx);
  assert.equal(rest.length, 0);
  assert.equal(f.severity, 'warn');
  assert.equal(C126.mode, 'report-only');
  assert.match(f.message, /rule A, ADR 0103/);
  assert.match(f.fix, /run_cases/);
});

// The review of 2026-11-15 (probe.mjs): false positives and the main false negatives of the first version.
const probe = (file, text) => stepSites(text, file).map((s) => s.rule);

test('C126 rule A: an impl block is not a loop, and an HTTP status is not a program', () => {
  assert.deepEqual(probe('tests/a.rs', 'use std::process::Command;\nstruct S;\nimpl Run for S {\n    fn go(&self) { let o = Command::new("x").output().unwrap(); }\n}\n'), []);
  assert.deepEqual(probe('tests/a.rs', 'use std::process::Command;\nfn f<F>() where F: for<\'a> Fn(&\'a str) {\n    let o = Command::new("x").output().unwrap();\n}\n'), []);
  assert.deepEqual(probe('tests/b.rs', 'use std::process::Command;\nfn t() {\n    for u in urls { let s = client.get(u).send().unwrap().status(); }\n}\n'), []);
  // A command's status in a loop still counts: on a Command chain, or on a binding made from one.
  assert.deepEqual(probe('tests/b.rs', 'use std::process::Command;\nfn t() {\n    for a in xs { Command::new("git").arg(a).status().unwrap(); }\n}\n'), ['A']);
  assert.deepEqual(probe('tests/b.rs', 'use std::process::Command;\nfn t() {\n    let mut git = Command::new("git");\n    for a in xs { git.status().unwrap(); }\n}\n'), ['A']);
});

test('C126 rule C: a spawn whose wait comes after an assertion, or another child\'s wait, still leaks', () => {
  assert.deepEqual(probe('tests/c.rs', 'use std::process::Command;\nfn t() {\n    let mut c = Command::new("sleep").spawn().unwrap();\n    assert!(false);\n    c.wait().unwrap();\n}\n'), ['C']);
  assert.deepEqual(probe('tests/d.rs', 'use std::process::Command;\nfn t() {\n    let leak = Command::new("daemon").spawn().unwrap();\n    let mut b = Command::new("x").spawn().unwrap(); b.wait().unwrap();\n}\n'), ['C']);
  assert.deepEqual(probe('tests/d.rs', 'use std::process::Command;\nfn t() {\n    let mut b = Command::new("x").spawn().unwrap();\n    b.wait().unwrap();\n    assert!(true);\n}\n'), []);
  assert.deepEqual(probe('tests/d.rs', 'use std::process::Command;\nfn t() {\n    let s = Command::new("x").spawn().unwrap().wait().unwrap();\n}\n'), []);
});

test('C126 rule B (Node): a timers/promises sleep or a pause held in a name is a fixed poll too', () => {
  assert.deepEqual(probe('scripts/g.test.mjs', "import { setTimeout as sleep } from 'node:timers/promises';\nwhile (!done()) await sleep(100);\n"), ['B']);
  assert.deepEqual(probe('scripts/h.test.mjs', 'const P = 100;\nfor (let i = 0; i < 50 && !ok(); i++) await new Promise((r) => setTimeout(r, P));\n'), ['B']);
});

test('C126 rule C (Node): a word "Reaper" is not a reaper; a call is', () => {
  assert.deepEqual(probe('scripts/i.test.mjs', "// see Reaper\nconst Reaper = null;\nspawn('x', [], { detached: true });\n"), ['C']);
  assert.deepEqual(probe('scripts/i.test.mjs', "const r = new Reaper('t');\nspawn('x', [], { detached: true });\n"), []);
});

test('C126 rule C: a spawn handed to a guard by a wrapper, a field, a match or a call is owned; one meant to fail is fine', () => {
  const c = (body) => probe('tests/e.rs', `use std::process::Command;\nfn t() {\n${body}\n}\n`);
  assert.deepEqual(c('    let mut child = Reaped(c.spawn().unwrap());'), []);
  assert.deepEqual(c('    let s = Sleeper { child: c.spawn().unwrap() };'), []);
  assert.deepEqual(c('    let child = Command::new("sleep").spawn();\n    match child { Ok(mut c) => { c.kill().unwrap(); } Err(_) => {} }'), []);
  assert.deepEqual(c('    let child = Command::new("sleep").spawn().unwrap();\n    end_it(child);'), []);
  assert.deepEqual(c('    assert!(command("no-such-tool").spawn().is_err());'), []);
});
