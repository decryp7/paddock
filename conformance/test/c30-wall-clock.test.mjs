import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mask, wallClockAsserts } from '../src/checks/C30-wall-clock.mjs';
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
