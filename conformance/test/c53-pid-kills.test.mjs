import assert from 'node:assert/strict';
import { test } from 'node:test';
import { pidKills } from '../src/checks/C53-pid-kills.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const lines = (text) => pidKills(text).map((k) => k.line);

test('C53 bad: Windows ends, kill programs and open follow-ups', () => {
  assert.deepEqual(lines('fn a(h: HANDLE) {\n    unsafe { TerminateProcess(h, 1) };\n}\n'), [2]);
  assert.deepEqual(lines('fn a(h: HANDLE) {\n    unsafe { TerminateJobObject(h, 1) };\n}\n'), [2]);
  assert.deepEqual(lines('fn a() {\n    Command::new("taskkill").args(["/T", "/F"]);\n}\n'), [2]);
  assert.deepEqual(lines('fn exe() -> PathBuf {\n    root.join("taskkill.exe")\n}\n'), [2]);
  assert.deepEqual(lines('fn a() {\n    std::process::Command::new("kill").arg("-9");\n    paddock_proc::command("pkill");\n}\n'), [2, 3]);
  assert.deepEqual(lines('fn a() {\n    ps.arg("Stop-Process -Id 42 -Force");\n    Command::new("/bin/kill");\n}\n'), [2, 3]);
  const marked = pidKills('#[expect(clippy::disallowed_methods, reason = "ADR 0033 WP-5: killpg after reap")]\nfn a() {}\n// ADR 0033 WP-6: parent pids\n');
  assert.deepEqual(marked.map((k) => [k.line, k.waived]), [[1, undefined], [3, undefined]]);
  assert.match(marked[0].what, /WP-5/);
  assert.deepEqual(lines('#![allow(clippy::unwrap_used, clippy::disallowed_methods)]\nfn a() {}\n'), [1]);
  assert.deepEqual(lines('#![cfg_attr(test, allow(clippy::disallowed_methods))]\n'), [1]);
  assert.deepEqual(lines('#[allow(clippy::disallowed_methods)]\nfn a() {}\n// #![allow(clippy::disallowed_methods)]\n'), [], 'an item-level allow and a comment are fine');
  // conformance: pid-kill does not waive a follow-up marker
  assert.equal(pidKills('// conformance: pid-kill — no\n// ADR 0033 WP-6: x\n')[0].waived, undefined);
});

test('C53 good: Child::kill, a lookup without a kill, comments and strings', () => {
  assert.deepEqual(lines('fn a(c: &mut Child) {\n    let _ = c.kill();\n}\n'), []);
  assert.deepEqual(lines('fn a() {\n    let alive = sys.process(Pid::from_u32(pid)).is_some();\n}\n'), []);
  assert.deepEqual(lines('// libc::kill(p, 9) and Command::new("taskkill")\nfn a() {\n    let s = "TerminateProcess(h, 1) taskkill";\n}\n'), []);
  assert.deepEqual(lines('fn a() {\n    paddock_proc::kill_tree(id);\n    paddock_proc::end(id);\n}\n'), []);
  // Clippy's disallowed-methods covers these now.
  assert.deepEqual(lines('fn a(p: i32) {\n    unsafe { libc::kill(p, 9) };\n    if let Some(p) = sys.process(Pid::from_u32(pid)) { p.kill(); }\n}\n'), []);
});

test('C53 waivers: with a reason the site passes, without one it is reported', () => {
  const k = pidKills('fn a(h: HANDLE) {\n    // conformance: pid-kill — a handle we own\n    unsafe { TerminateProcess(h, 1) };\n}\n');
  assert.equal(k[0].waived, 'reason');
  assert.equal(pidKills('fn a(h: HANDLE) {\n    unsafe { TerminateProcess(h, 1) }; // conformance: pid-kill\n}\n')[0].waived, 'no-reason');
});

test('C53 through the runner: an error per bare-pid kill, none in the one implementation or for a waived site', async () => {
  const bad = 'fn a() {\n    Command::new("taskkill");\n}\n';
  const waived = 'fn a(h: HANDLE) {\n    // conformance: pid-kill — a handle we own\n    unsafe { TerminateProcess(h, 1) };\n}\n';
  const bare = 'fn a(h: HANDLE) {\n    // conformance: pid-kill\n    unsafe { TerminateProcess(h, 1) };\n}\n';
  const f = await findingsOf(
    'C53',
    makeApp({ 'crates/a/src/bad.rs': bad, 'crates/paddock-proc/src/sys.rs': bad, 'crates/a/src/w.rs': waived, 'crates/a/src/b.rs': bare }),
  );
  assert.deepEqual(f.map((x) => [x.file, x.severity]).sort(), [['crates/a/src/b.rs', 'error'], ['crates/a/src/bad.rs', 'error']]);
  assert.match(f.find((x) => x.file.endsWith('bad.rs')).fix, /ProcessId/);
  assert.match(f.find((x) => x.file.endsWith('b.rs')).message, /no reason/);
});

test('C53 leaves vendored third-party code to its own upstream (a vendor/ folder)', async () => {
  const bad = 'fn a() {\n    Command::new("taskkill");\n}\n';
  const f = await findingsOf('C53', makeApp({ 'apps/x/vendor/dep/src/win.rs': bad, 'crates/a/src/bad.rs': bad }));
  assert.deepEqual(f.map((x) => x.file), ['crates/a/src/bad.rs']);
});
