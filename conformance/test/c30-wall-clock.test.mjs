import assert from 'node:assert/strict';
import { test } from 'node:test';
import { graceAsserts, mask, threadSleeps, tsWallClockAsserts, wallClockAsserts } from '../src/checks/C30-wall-clock.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const wrap = (body) => `use std::time::*;\n#[test]\nfn it() {\n${body}\n}\n`;

test('C30 bad: elapsed, Instant arithmetic and tainted variables compared with a Duration', () => {
  assert.equal(wallClockAsserts(wrap('    let t = Instant::now();\n    work();\n    assert!(t.elapsed() < Duration::from_millis(700));')).length, 1);
  assert.equal(wallClockAsserts(wrap('    let t = Instant::now();\n    let took = t.elapsed();\n    assert!(took <= Duration::from_secs(1), "slow");')).length, 1);
  assert.equal(wallClockAsserts(wrap('    assert!(SystemTime::now().duration_since(a).unwrap().as_secs() > 3);')).length, 1);
  assert.equal(wallClockAsserts('#[tokio::test]\nasync fn a() { let s = Instant::now(); assert!(s.elapsed() < Duration::from_secs(2)); }\n').length, 1);
  assert.equal(wallClockAsserts('#[test]\nfn multi() {\n    assert!(\n        start.elapsed()\n            < Duration::from_secs(2)\n    );\n}\n').length, 1, 'a multi-line assertion is found too');
  assert.equal(wallClockAsserts('#[test]\nfn multi() {\n    assert!(\n        Instant::now() - s\n            < Duration::from_secs(2)\n    );\n}\n')[0]?.line, 3);
});

test('C30 good: FakeClock, counts, non-test code, comments and strings', () => {
  assert.deepEqual(wallClockAsserts(wrap('    let clock = FakeClock::new();\n    clock.advance(Duration::from_secs(5));\n    assert_eq!(clock.monotonic(), Duration::from_secs(5));')), []);
  assert.deepEqual(wallClockAsserts('fn helper() { let t = Instant::now(); assert!(t.elapsed() < Duration::from_secs(1)); }\n'), []);
  assert.deepEqual(wallClockAsserts(wrap('    // assert!(t.elapsed() < Duration::from_secs(1));\n    let s = "assert!(t.elapsed() < Duration::ZERO)";\n    let t = Instant::now();\n    work();\n    println!("{:?}", t.elapsed());')), []);
  assert.deepEqual(wallClockAsserts(wrap('    let t = Instant::now();\n    assert!(items == 3);')), []);
  // An instant used as the base of computed instants is deterministic, not a measurement.
  assert.deepEqual(wallClockAsserts(wrap('    let t0 = Instant::now();\n    assert!(tracker.terminated("main", t0 + Duration::from_secs(1)));\n    assert_eq!(t.current(t0 + Duration::from_secs(2)), None);')), []);
});

test('C30 hang guards: waived from 10 s, too tight below, a comment line above counts', () => {
  const [ok] = wallClockAsserts(wrap('    let t = Instant::now();\n    // conformance: hang-guard\n    assert!(t.elapsed() < Duration::from_secs(60));'));
  assert.equal(ok.guard, 60_000);
  const [tight] = wallClockAsserts(wrap('    let t = Instant::now();\n    assert!(t.elapsed() < Duration::from_secs(2)); // conformance: hang-guard'));
  assert.equal(tight.guard, 2000);
});

test('C30 through the runner: a warning per assertion, none for a clean or guarded file', async () => {
  const bad = wrap('    let t = Instant::now();\n    assert!(t.elapsed() < Duration::from_millis(700));');
  const f = await findingsOf('C30', makeApp({ 'crates/a/tests/bad.rs': bad, 'crates/a/tests/good.rs': wrap('    assert!(true);') }));
  assert.equal(f.length, 1);
  assert.equal(f[0].severity, 'warn');
  assert.equal(f[0].file, 'crates/a/tests/bad.rs');
  const guarded = wrap('    let t = Instant::now();\n    // conformance: hang-guard\n    assert!(t.elapsed() < Duration::from_secs(30));');
  assert.deepEqual(await findingsOf('C30', makeApp({ 'crates/a/tests/g.rs': guarded })), []);
  const tight = wrap('    let t = Instant::now();\n    // conformance: hang-guard\n    assert!(t.elapsed() < Duration::from_secs(3));');
  const g = await findingsOf('C30', makeApp({ 'crates/a/tests/t.rs': tight }));
  assert.match(g[0].message, /too tight/);
});

test('mask keeps newlines and blanks comments and strings', () => {
  const m = mask('a // x\n"b{" /* c\n d */ e');
  assert.equal(m.split('\n').length, 3);
  assert.ok(!m.includes('{') && !m.includes('x'));
});

const sleepy = (body) => `use std::{sync::*, thread, time::Duration};\n#[test]\nfn it() {\n${body}\n}\n`;

test('C30 sleeps: a thread::sleep in a test that uses threads is flagged; alone, or waived, it is not', () => {
  assert.equal(threadSleeps(sleepy('    let h = thread::spawn(|| work());\n    thread::sleep(Duration::from_millis(50));\n    h.join().unwrap();')).length, 1);
  assert.equal(threadSleeps(sleepy('    let flag = Arc::new(AtomicBool::new(false));\n    std::thread::sleep(Duration::from_millis(5));')).length, 1);
  assert.deepEqual(threadSleeps(sleepy('    thread::sleep(Duration::from_millis(5));\n    assert!(true);')), [], 'no threads in the test');
  const [waived] = threadSleeps(sleepy('    let h = thread::spawn(|| work());\n    // conformance: hang-guard — the OS has no event for this\n    thread::sleep(Duration::from_millis(5));'));
  assert.equal(waived.waived, 'reason');
  const [bare] = threadSleeps(sleepy('    let h = thread::spawn(|| work());\n    // conformance: hang-guard\n    thread::sleep(Duration::from_millis(5));'));
  assert.equal(bare.waived, 'no-reason', 'the comment alone is not a waiver');
  const [short] = threadSleeps(sleepy('    let h = thread::spawn(|| work());\n    thread::sleep(Duration::from_millis(5)); // conformance: hang-guard - ok'));
  assert.equal(short.waived, 'no-reason', 'a one-word reason is not one');
  assert.deepEqual(threadSleeps('fn helper() { thread::spawn(|| thread::sleep(d)); }\n'), [], 'not a test');
  assert.deepEqual(threadSleeps(sleepy('    let h = thread::spawn(|| work());\n    // thread::sleep(d);\n    h.join().unwrap();')), []);
});

test('C30 TypeScript: Date.now() / performance.now() differences asserted are flagged; fake timers and plain use are not', () => {
  const spec = (body) => `import { it, expect } from 'vitest';\nit('x', async () => {\n${body}\n});\n`;
  assert.equal(tsWallClockAsserts(spec('  const t0 = Date.now();\n  await work();\n  expect(Date.now() - t0).toBeLessThan(700);')).length, 1);
  assert.equal(tsWallClockAsserts(spec('  const start = performance.now();\n  await work();\n  const took = performance.now() - start;\n  expect(took).toBeLessThanOrEqual(50);')).length, 1);
  assert.equal(tsWallClockAsserts(spec('  const a = Date.now();\n  const b = Date.now();\n  expect(b - a).toBeGreaterThan(5);')).length, 1);
  assert.deepEqual(tsWallClockAsserts(spec('  vi.useFakeTimers();\n  const t0 = Date.now();\n  vi.advanceTimersByTime(5);\n  expect(Date.now() - t0).toBeLessThan(700);')), []);
  assert.deepEqual(tsWallClockAsserts(spec('  await page.clock.install();\n  const t0 = Date.now();\n  expect(Date.now() - t0).toBeLessThan(700);')), []);
  assert.deepEqual(tsWallClockAsserts(spec('  const id = Date.now();\n  expect(id).toBeGreaterThan(0);\n  expect(items.length - 1).toBeLessThan(3);')), []);
  assert.deepEqual(tsWallClockAsserts(spec('  // expect(Date.now() - t0).toBeLessThan(700);\n  const s = "Date.now() - t0";')), []);
  const [guard] = tsWallClockAsserts(spec('  const t0 = Date.now();\n  // conformance: hang-guard\n  expect(Date.now() - t0).toBeLessThan(60_000);'));
  assert.equal(guard.guard, 60_000);
  const [tight] = tsWallClockAsserts(spec('  const t0 = Date.now();\n  expect(Date.now() - t0).toBeLessThan(2000); // conformance: hang-guard'));
  assert.equal(tight.guard, 2000);
});

test('C30 through the runner: sleeps in Rust and wall-clock expects in specs are warnings, other files are left alone', async () => {
  const rs = sleepy('    let h = thread::spawn(|| work());\n    thread::sleep(Duration::from_millis(50));\n    h.join().unwrap();');
  const ts = "it('x', () => {\n  const t0 = Date.now();\n  expect(Date.now() - t0).toBeLessThan(700);\n});\n";
  const f = await findingsOf('C30', makeApp({ 'crates/a/tests/s.rs': rs, 'app/src/a.spec.ts': ts, 'app/e2e/b.ts': ts, 'app/src/app.ts': ts }));
  assert.deepEqual(f.map((x) => x.file).sort(), ['app/e2e/b.ts', 'app/src/a.spec.ts', 'crates/a/tests/s.rs']);
  assert.ok(f.every((x) => x.severity === 'warn'));
});

test('C30 through the runner: a sleep waiver needs a reason', async () => {
  const body = (c) => sleepy(`    let h = thread::spawn(|| work());\n    ${c}\n    thread::sleep(Duration::from_millis(50));\n    h.join().unwrap();`);
  assert.deepEqual(await findingsOf('C30', makeApp({ 'crates/a/tests/a.rs': body('// conformance: hang-guard — the kernel gives no event for a mtime tick') })), []);
  const f = await findingsOf('C30', makeApp({ 'crates/a/tests/a.rs': body('// conformance: hang-guard') }));
  assert.match(f[0].message, /gives no reason/);
});

test('C30 grace: assert! over close() or close_gracefully() is flagged, waivers and other calls are not', () => {
  assert.equal(graceAsserts(wrap('    assert!(b.close(), "ends by itself");')).length, 1);
  assert.equal(graceAsserts(wrap('    assert!(b.close_gracefully(Duration::from_secs(5)));'))[0].waiver, null);
  const [g] = graceAsserts(wrap('    // conformance: hang-guard — only stops a browser that never exits\n    assert!(b.close_gracefully(Duration::from_secs(120)));'));
  assert.deepEqual([g.waiver, g.limitMs], ['hang-guard', 120_000]);
  assert.equal(graceAsserts(wrap('    // conformance: grace-free — it ended already\n    assert!(b.close(), "a second close");'))[0].waiver, 'grace-free');
  // A hang-guard comment does not waive a bare close() (its grace is fixed), and a grace-free one needs a reason.
  assert.equal(graceAsserts(wrap('    // conformance: hang-guard\n    assert!(b.close());'))[0].waiver, null);
  assert.equal(graceAsserts(wrap('    // conformance: grace-free\n    assert!(b.close());'))[0].waiver, null);
  assert.deepEqual(graceAsserts(wrap('    b.close();\n    assert!(!dir.exists());')), []);
  assert.deepEqual(graceAsserts('fn helper(b: &mut B) { assert!(b.close()); }\n'), []);
  assert.deepEqual(graceAsserts(wrap('    // assert!(b.close());\n    let s = "assert!(b.close())";')), []);
});

test('C30 grace: close(id), negated asserts and other close_* names are not the browser close', () => {
  assert.deepEqual(graceAsserts(wrap('    assert!(s.close(id));')), []);
  assert.deepEqual(graceAsserts(wrap('    assert_eq!(store.close(7), Ok(()));')), []);
  assert.deepEqual(graceAsserts(wrap('    assert!(!b.close_gracefully(Duration::ZERO));')), []);
  assert.deepEqual(graceAsserts(wrap('    assert!( ! b.close_gracefully(Duration::ZERO), "forced");')), []);
  assert.deepEqual(graceAsserts(wrap('    assert!(window.close_all());')), []);
});

test('C30 grace: a guarded close_gracefully of under 10 s is too tight', async () => {
  const tight = wrap('    // conformance: hang-guard\n    assert!(b.close_gracefully(Duration::from_millis(200)));');
  assert.equal(graceAsserts(tight)[0].limitMs, 200);
  const f = await findingsOf('C30', makeApp({ 'crates/a/tests/t.rs': tight }));
  assert.equal(f.length, 1);
  assert.match(f[0].message, /too tight/);
  const ok = wrap('    // conformance: hang-guard\n    assert!(b.close_gracefully(Duration::from_secs(120)));');
  assert.deepEqual(await findingsOf('C30', makeApp({ 'crates/a/tests/t.rs': ok })), []);
});

test('C30 grace through the runner: a warning for an unguarded assert over close()', async () => {
  const f = await findingsOf('C30', makeApp({ 'crates/a/tests/b.rs': wrap('    assert!(b.close());'), 'crates/a/tests/ok.rs': wrap('    b.close();') }));
  assert.equal(f.length, 1);
  assert.equal(f[0].file, 'crates/a/tests/b.rs');
});
