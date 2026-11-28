import assert from 'node:assert/strict';
import { test } from 'node:test';
import { recordedPidSites } from '../src/checks/C128-recorded-pid.mjs';

const lines = (text, file = 'crates/a/tests/it/x.rs') => recordedPidSites(text, file).map((s) => s.line);

test('C128: a pid parsed from a file or output, then ProcessId::of (run 8118 and its review)', () => {
  // Spotter's helper test before f224dd0b4: the pid file read and its identity in one closure.
  assert.deepEqual(lines('fn t() {\n    let id = poll_for("pid", || {\n        let pid = std::fs::read_to_string(f).ok()?.trim().parse::<u32>().ok()?;\n        paddock_proc::ProcessId::of(pid)\n    });\n}\n'), [4]);
  // paddock-chromium's launcher test: a pid printed by `sh -c "... & echo $!"`.
  assert.deepEqual(lines('fn t() {\n    let pid: u32 = String::from_utf8_lossy(&out.stdout).trim().parse().unwrap();\n    started.push(ProcessId::of(pid).unwrap());\n}\n'), [3]);
  assert.deepEqual(lines('fn t() {\n    let (pid, dir) = (ProcessId::of(pid.parse().unwrap()).unwrap(), x);\n}\n'), [2]);
  assert.deepEqual(lines('fn t() {\n    let id = text.trim().parse::<u32>().ok().and_then(paddock_proc::ProcessId::of);\n}\n'), [2], 'passed as a function');
});

test('C128: a child the test holds, other functions, production code, comments, strings and waivers are fine', () => {
  assert.deepEqual(lines('fn t() {\n    let child = cmd.spawn().unwrap();\n    let id = ProcessId::of(child.id()).unwrap();\n}\n'), []);
  assert.deepEqual(lines('fn a() {\n    let pid: u32 = s.parse().unwrap();\n}\nfn b(pid: u32) {\n    let id = ProcessId::of(pid);\n}\n'), [], 'a binding in another function is not followed (a known gap)');
  assert.deepEqual(lines('fn t() {\n    let pid = s.parse().unwrap();\n    let pid = child.id();\n    let id = ProcessId::of(pid);\n}\n'), [], 'the last binding counts');
  assert.deepEqual(lines('fn t() {\n    // ProcessId::of(s.parse().unwrap())\n    let s = "ProcessId::of(x.parse())";\n}\n'), []);
  assert.deepEqual(lines('fn t() {\n    // conformance: recorded-pid — the writer blocks on stdin, which this test holds\n    let id = ProcessId::of(s.parse().unwrap());\n}\n'), []);
  assert.deepEqual(lines('fn t() {\n    let id = ProcessId::of_recorded(s.parse().unwrap(), ms);\n}\n'), [], 'of_recorded is the fix');
  assert.deepEqual(lines('fn t() {\n    let id = ProcessId::of(s.parse().unwrap());\n}\n', 'crates/a/src/lib.rs'), [], 'production code without a test module');
  assert.deepEqual(lines('fn p() {\n    let id = ProcessId::of(s.parse().unwrap());\n}\n#[cfg(test)]\nmod tests {\n    fn t() {\n        let id = ProcessId::of(s.parse().unwrap());\n    }\n}\n', 'crates/a/src/lib.rs'), [7], 'a test module from its start');
  assert.deepEqual(lines('fn t() {\n    let id = ProcessId::of(s.parse().unwrap());\n}\n', 'crates/paddock-testkit-start/src/recorded.rs'), [], 'the kit itself');
});

test('C128: a test module behind cfg(all(unix, test)) or cfg(any(test, ...)) is read too (review of fix/test-pid-reader)', () => {
  const body = 'mod tests {\n    fn t() {\n        let id = ProcessId::of(s.parse().unwrap());\n    }\n}\n';
  assert.deepEqual(lines(`fn p() {}\n#[cfg(all(unix, test))]\n${body}`, 'crates/a/src/lib.rs'), [5]);
  assert.deepEqual(lines(`fn p() {}\n#[cfg(any(test, feature = "testing"))]\n${body}`, 'crates/a/src/lib.rs'), [5]);
  assert.deepEqual(lines(`fn p() {}\n#[cfg(all(unix, not(windows)))]\n${body}`, 'crates/a/src/lib.rs'), [], 'a cfg without test is production code');
});

test('C128: by item, in both directions: production code after a test module is not read, a test fn or a second test module is', () => {
  const site = 'let id = ProcessId::of(s.parse().unwrap());';
  const prod = `fn p() {\n    ${site}\n}\n`;
  const testMod = `#[cfg(test)]\nmod tests {\n    fn t() {\n        ${site}\n    }\n}\n`;
  // Over-flag direction: production code AFTER the test module was read as test code (everything from the module onwards).
  assert.deepEqual(lines(`${testMod}${prod}`, 'crates/a/src/lib.rs'), [4], 'only the line inside the test module');
  // Hide direction: a test-only fn and a later test module in the same file are test code too.
  const helper = `#[cfg(all(unix, test))]\nfn helper() {\n    ${site}\n}\n`;
  assert.deepEqual(lines(`${prod}${helper}${testMod}`, 'crates/a/src/lib.rs'), [6, 11], 'the production site on line 2 is not read');
});

test('C128: paddock-proc\'s own tests take no "cycle" waiver: the identity helpers live in paddock-proc (test-support)', async () => {
  // The old shape of the four paddock-proc sites, without its waiver, is flagged (the check goes red when the helpers are not used).
  const old = 'fn t() {\n    let out = x;\n    ids.push(ProcessId::of(p.parse().unwrap()).unwrap());\n}\n';
  assert.deepEqual(lines(old, 'crates/paddock-proc/src/guardian/unix/tests.rs'), [3]);
  assert.deepEqual(lines(old, 'crates/paddock-proc/tests/it/scope.rs'), [3]);
  // And no file of the crate carries the excuse any more.
  const { readdirSync, readFileSync, statSync } = await import('node:fs');
  const { join, dirname } = await import('node:path');
  const { fileURLToPath } = await import('node:url');
  const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'crates', 'paddock-proc');
  const walk = (d) => readdirSync(d).flatMap((n) => {
    const p = join(d, n);
    return statSync(p).isDirectory() ? (n === 'target' ? [] : walk(p)) : p.endsWith('.rs') ? [p] : [];
  });
  const waived = walk(root).filter((f) => /recorded-pid[^\n]*\(a cycle\)/.test(readFileSync(f, 'utf8')));
  assert.deepEqual(waived, [], 'use paddock_proc::recorded (descendant_of, printed) instead of a "cannot use paddock-testkit-start" waiver');
});
