import assert from 'node:assert/strict';
import { test } from 'node:test';
import { windowedTestFindings } from '../src/checks/C51-windowed-test-browser.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const t = (attr, body) => `${attr}\nasync fn it() {\n${body}\n}\n`;

test('C51 bad: a headful test browser without the gate (the off-screen Spotter test before the fix)', () => {
  const offScreen = t('#[tokio::test(flavor = "multi_thread", worker_threads = 2)]', '    let Some(b) = browser() else { return };\n    let r = ChromiumRenderer::new(base.path(), base.path()).with_browser(b);');
  assert.deepEqual(windowedTestFindings('crates/x/tests/real_browser.rs', offScreen).map((f) => [f.kind, f.name, f.line]), [['ungated', 'it', 2]]);
  const onScreen = t('#[test]', '    let mut o = LaunchOptions::new(d);\n    o.mode = Mode::OnScreen;\n    Chromium::launch(&b, &o).unwrap();');
  assert.equal(windowedTestFindings('tests/it/live.rs', onScreen)[0].kind, 'ungated');
});

test('C51 good: headless, gated, outside tests/, or the mode only in a comment or string', () => {
  const gated = t('#[tokio::test]', '    if !windowed_tests_here() {\n        return;\n    }\n    let r = ChromiumRenderer::new(a, b);');
  const headless = t('#[test]', '    let mut o = LaunchOptions::new(d);\n    o.mode = Mode::Headless;\n    let r = ChromiumRenderer::new(a, b).with_mode(Mode::Headless);');
  const driver = t('#[test]', '    let Some(exe) = test_browser() else { return };\n    let b = Chromium::launch(&exe, &LaunchOptions::default());');
  const comment = t('#[test]', '    // Mode::OffScreen is the default\n    let s = "Mode::OnScreen";');
  for (const text of [gated, headless, driver, comment]) assert.deepEqual(windowedTestFindings('crates/x/tests/real_browser.rs', text), []);
  // Unit tests of the launch arguments (no browser starts) live in src/, which the second rule does not scan.
  assert.deepEqual(windowedTestFindings('crates/x/src/launch.rs', t('#[test]', '    let a = args(p, Mode::OffScreen);')), []);
});

test('C51 bad: the test browser read around its finder; the finder itself may', () => {
  const direct = '#[test]\nfn it() {\n    let p = std::env::var("CHROMIUM_TEST_PATH").unwrap();\n}\n';
  assert.deepEqual(windowedTestFindings('app/src/browser/live.rs', direct).map((f) => [f.kind, f.line]), [['direct', 3]]);
  assert.deepEqual(windowedTestFindings('app/src/x.rs', 'let b = find_for_tests_with(Os::Mac, &env, &exists);').map((f) => f.kind), ['direct']);
  assert.deepEqual(windowedTestFindings('crates/d/src/discover.rs', direct + 'find_for_tests_with(os, &e, &x);'), []);
  assert.deepEqual(windowedTestFindings('app/src/x.rs', '// reads CHROMIUM_TEST_PATH: var("CHROMIUM_TEST_PATH")'), []);
});

test('C51 through the runner: one error per ungated test or direct read', async () => {
  const root = makeApp({
    'crates/r/tests/real_browser.rs': t('#[test]', '    let r = ChromiumRenderer::new(a, b);'),
    'crates/r/tests/ok.rs': t('#[test]', '    if !windowed_tests_here() { return; }\n    let r = ChromiumRenderer::new(a, b);'),
    'app/src/live.rs': 'fn f() { let _ = std::env::var_os("CHROMIUM_TEST_PATH"); }\n',
  });
  const found = await findingsOf('C51', root);
  assert.deepEqual(found.map((f) => [f.file, f.severity]).sort(), [['app/src/live.rs', 'error'], ['crates/r/tests/real_browser.rs', 'error']]);
});
