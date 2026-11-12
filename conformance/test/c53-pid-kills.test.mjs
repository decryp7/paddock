import assert from 'node:assert/strict';
import { test } from 'node:test';
import { pidKills, pidLiveness } from '../src/checks/C53-pid-kills.mjs';
import { LIVENESS_BASELINE } from '../src/checks/C53-pid-kills.mjs';
import { lineHash } from '../src/checks/C71-test-timing.mjs';
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
  assert.deepEqual(lines('fn a() {\n    let alive = sys.process(Pid::from_u32(pid)).is_some();\n}\n'), [], 'a kill check leaves liveness to pidLiveness');
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
  const f = await findingsOf('C53', makeApp({ 'apps/x/vendor/dep/src/win.rs': bad, 'crates/a/src/bad.rs': bad, 'crates/x/src/vendor/a.rs': bad }));
  assert.deepEqual(f.map((x) => x.file).sort(), ['crates/a/src/bad.rs', 'crates/x/src/vendor/a.rs'], 'a first-party folder named vendor is still checked');
});

const live = (text, file = 'crates/a/src/x.rs') => pidLiveness(text, file).map((k) => k.line);

test('C53 liveness bad: a raw pid asked whether it is alive', () => {
  assert.deepEqual(live('fn a(pid: u32) -> bool {\n    paddock_proc::is_alive(pid)\n}\n'), [2]);
  assert.deepEqual(live('fn a(pid: u32) -> bool {\n    paddock_proc::may_be_alive(pid)\n}\n'), [2]);
  assert.deepEqual(live('fn a(pid: u32) {\n    let p = paddock_proc::presence(pid);\n}\n'), [2]);
  assert.deepEqual(live('use paddock_proc::is_alive;\nfn a(pid: u32) -> bool {\n    is_alive(pid)\n}\n'), [3]);
  assert.deepEqual(live('fn a(pid: u32) -> bool {\n    crate::is_alive(pid)\n}\n', 'crates/paddock-proc/src/testchild.rs'), [2]);
  assert.deepEqual(live('fn a(pid: u32) -> bool {\n    is_alive(pid)\n}\n', 'crates/paddock-proc/src/tree.rs'), [2]);
  assert.deepEqual(live('fn a(pid: i32) -> bool {\n    unsafe { libc::kill(pid, 0) == 0 }\n}\n'), [2]);
  assert.deepEqual(live('fn a(pid: u32) {\n    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };\n}\n'), [2]);
  assert.deepEqual(live('fn a(pid: u32) -> bool {\n    sys.process(sysinfo::Pid::from_u32(pid)).is_some()\n}\n'), [2]);
});

test('C53 liveness good: a ProcessId, other uses of the names, comments, strings and the implementation', () => {
  assert.deepEqual(live('fn a(id: ProcessId) -> bool {\n    id.is_alive()\n}\n'), []);
  assert.deepEqual(live('fn a(id: ProcessId) {\n    let p = id.presence();\n}\n'), []);
  assert.deepEqual(live('pub fn is_alive(pid: u32) -> bool {\n    true\n}\n'), [], 'a definition is not a call');
  assert.deepEqual(live('fn a(app: &App) {\n    let seen = presence(app);\n}\n'), [], 'a bare presence() of another crate is not the pid one');
  assert.deepEqual(live('// paddock_proc::is_alive(pid) and kill(pid, 0)\nfn a() { let s = "OpenProcess(x, 0, pid)"; }\n'), []);
  assert.deepEqual(live('fn a(c: &mut Child) {\n    let _ = c.kill();\n    unsafe { libc::kill(p, 9) };\n}\n'), [], 'a real signal is the kill check, not liveness');
  assert.deepEqual(live('fn a(pid: i32) -> bool {\n    unsafe { libc::kill(pid, 0) == 0 }\n}\n', 'crates/paddock-proc/src/sys.rs'), [], 'the one implementation');
});

test('C53 liveness precision: signals, the own pid, aliases and globs', () => {
  assert.deepEqual(live('fn a(pid: i32) {\n    unsafe { libc::kill(pid, libc::SIGTERM) };\n    kill(pid, Signal::SIGKILL);\n}\n'), [], 'a signal is the kill check');
  assert.deepEqual(live('fn a(pid: Pid) {\n    let _ = nix::sys::signal::kill(pid, None);\n}\n'), [2], 'nix kill with no signal is liveness');
  assert.deepEqual(live('fn a() {\n    let me = Pid::from_u32(std::process::id());\n    let me = sysinfo::Pid::from_u32(process::id());\n}\n'), [], 'the own pid');
  assert.deepEqual(live('use paddock_proc::is_alive as up;\nfn a(pid: u32) -> bool {\n    up(pid)\n}\n'), [3]);
  assert.deepEqual(live('use paddock_proc::{presence as look, ProcessId};\nfn a(pid: u32) {\n    look(pid);\n}\n'), [3]);
  assert.deepEqual(live('use paddock_proc::*;\nfn a(pid: u32) -> bool {\n    may_be_alive(pid)\n}\n'), [3]);
});

test('C53 liveness waivers: a reason passes, none is reported', () => {
  const ok = pidLiveness('fn a(pid: u32) -> bool {\n    // conformance: pid-liveness — the pid came from our own child handle\n    paddock_proc::is_alive(pid)\n}\n', 'crates/a/src/x.rs');
  assert.equal(ok[0].waived, 'reason');
  const bare = pidLiveness('fn a(pid: u32) -> bool {\n    paddock_proc::is_alive(pid) // conformance: pid-liveness\n}\n', 'crates/a/src/x.rs');
  assert.equal(bare[0].waived, 'no-reason');
});

const entry = (file, text) => ({ file, hash: lineHash(text) });

test('C53 through the runner: a liveness site is an error unless the baseline lists it; a stale entry warns', async () => {
  const src = 'fn a(pid: u32) -> bool {\n    paddock_proc::is_alive(pid)\n}\n';
  const baseline = JSON.stringify({ sites: [entry('crates/a/src/known.rs', '    paddock_proc::is_alive(pid)'), entry('crates/a/src/gone.rs', 'gone')] });
  const root = makeApp({ 'crates/a/src/known.rs': src, 'crates/a/src/new.rs': src, [LIVENESS_BASELINE]: baseline });
  const f = await findingsOf('C53', root, { options: { git: () => { throw new Error('no git'); } } });
  const by = (sev) => f.filter((x) => x.severity === sev).map((x) => x.file);
  assert.deepEqual(by('error'), ['crates/a/src/new.rs']);
  assert.match(f.find((x) => x.file.endsWith('new.rs')).fix, /ProcessId/);
  assert.ok(by('warn').includes('crates/a/src/gone.rs'), 'a stale entry is a warning');
});

test('C53 through the runner: a baseline that grew against the merge base is an error', async () => {
  const src = 'fn a(pid: u32) -> bool {\n    paddock_proc::is_alive(pid)\n}\n';
  const grown = JSON.stringify({ sites: [entry('crates/a/src/k.rs', '    paddock_proc::is_alive(pid)')] });
  const root = makeApp({ 'crates/a/src/k.rs': src, [LIVENESS_BASELINE]: grown });
  const git = (args) => (args[0] === 'show' ? JSON.stringify({ sites: [] }) : 'abc123\n');
  const f = await findingsOf('C53', root, { options: { git } });
  assert.ok(f.some((x) => x.severity === 'error' && x.file === LIVENESS_BASELINE && /grew/.test(x.message)));
});
