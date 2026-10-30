import assert from 'node:assert/strict';
import { test } from 'node:test';
import { codeWithStrings, windowedTestFindings } from '../src/checks/C55-windowed-test-browser.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const t = (attr, body, name = 'it') => `${attr}\nasync fn ${name}() {\n${body}\n}\n`;
const kinds = (path, text) => windowedTestFindings(path, text).map((f) => f.kind);

test('C55 bad: a window launched with a browser asked for headless (Spotter, paddock-chromium, src unit tests)', () => {
  const offScreen = t('#[tokio::test(flavor = "multi_thread", worker_threads = 2)]', '    let Some(b) = browser(Mode::Headless) else { return };\n    let r = ChromiumRenderer::new(base.path(), base.path()).with_browser(b);');
  assert.deepEqual(windowedTestFindings('crates/x/tests/real_browser.rs', 'fn browser(m: Mode) -> Option<Browser> { find_for_tests(m) }\n' + offScreen).map((f) => [f.kind, f.name, f.line]), [['ungated', 'it', 3]]);
  const finderInFile = 'fn b() -> Option<Browser> { find_for_tests(Window::Headless) }\n';
  for (const body of [
    '    let mut o = LaunchOptions::new(d);\n    o.mode = Mode::OnScreen;\n    Chromium::launch(&b().unwrap(), &o).unwrap();',
    '    let o = LaunchOptions { window: Window::OffScreen, ..base };\n    Chromium::launch(&b().unwrap(), &o).unwrap();',
    '    Chromium::launch(&b().unwrap(), &LaunchOptions::default()).unwrap();',
    '    let w = Window::OnScreen(None);\n    go(b(), w);',
  ]) {
    // In a src/ unit test as well as under tests/.
    assert.deepEqual(kinds('crates/x/src/launch.rs', finderInFile + t('#[test]', body)), ['ungated'], body);
    assert.deepEqual(kinds('crates/x/tests/it/live.rs', finderInFile + t('#[test]', body)), ['ungated'], body);
  }
});

test('C55 good: headless, a finder asked for the window, Debrief\'s windowed test_browser, or no test browser at all', () => {
  const finder = 'fn b(m: Mode) -> Option<Browser> { find_for_tests(m) }\n';
  for (const body of [
    '    let Some(b) = browser(Mode::OffScreen) else { return };\n    let r = ChromiumRenderer::new(a, c).with_browser(b);',
    '    let Some(b) = find_for_tests(spotter_render::launch::Mode::OnScreen) else { return };\n    let mut o = LaunchOptions::new(d);\n    o.mode = Mode::OnScreen;',
    '    let mut o = LaunchOptions::new(d);\n    o.mode = Mode::Headless;\n    Chromium::launch(&b(Mode::Headless).unwrap(), &o).unwrap();',
    '    let Some(exe) = require_browser() else { return };\n    let b = Chromium::launch(&exe, &headless_options(base.path()));',
    '    let Some(exe) = test_browser() else { return };\n    let b = Chromium::launch(&exe, &LaunchOptions::default());',
    '    // Mode::OffScreen is the default\n    let s = "Mode::OnScreen";',
  ]) {
    assert.deepEqual(kinds('crates/x/tests/real_browser.rs', finder + t('#[test]', body)), [], body);
  }
  // Fake-browser tests: the file never gets a test browser, so their windowed options launch nothing real.
  const fake = t('#[test]', '    let f = FakeBrowser::install(d);\n    let mut o = LaunchOptions::new(p);\n    o.window = Window::OffScreen;\n    Chromium::launch_with(&f.exe(), &o, Sandbox::On).unwrap();');
  assert.deepEqual(kinds('crates/paddock-chromium/src/launch/tests.rs', fake), []);
  // The finder's own file.
  assert.deepEqual(kinds('crates/paddock-chromium/src/discover.rs', 'const TEST_BROWSER_ENV: &str = "CHROMIUM_TEST_PATH";\nfn find_for_tests_with() {}'), []);
});

test('C55 bad: the test browser read around its finder, also over lines and through the constant', () => {
  assert.deepEqual(windowedTestFindings('app/src/browser/live.rs', '#[test]\nfn it() {\n    let p = std::env::var("CHROMIUM_TEST_PATH").unwrap();\n}\n').map((f) => [f.kind, f.line]), [['direct', 3]]);
  assert.deepEqual(kinds('app/src/x.rs', 'let p = std::env::var(\n    "SPOTTER_TEST_BROWSER",\n);'), ['direct']);
  assert.deepEqual(kinds('app/src/x.rs', 'let d = env::var_os("SPOTTER_TEST_BROWSER_DIR");'), ['direct']);
  assert.deepEqual(kinds('app/src/x.rs', 'let p = std::env::var(paddock_chromium::discover::TEST_BROWSER_ENV);'), ['direct']);
  assert.deepEqual(kinds('app/src/x.rs', 'let b = find_for_tests_with(Os::Mac, &env, &exists);'), ['direct']);
  // Only src/discover.rs is the finder: a discover.rs elsewhere is not exempt.
  assert.deepEqual(kinds('crates/x/tests/discover.rs', 'let p = var("CHROMIUM_TEST_PATH");'), ['direct']);
  assert.deepEqual(kinds('crates/x/src/browser/discover_helpers.rs', 'let p = var("CHROMIUM_TEST_PATH");'), ['direct']);
});

test('C55 good: comments, other variables, and names that only look alike', () => {
  assert.deepEqual(kinds('app/src/x.rs', '// reads CHROMIUM_TEST_PATH: var("CHROMIUM_TEST_PATH")\n/* TEST_BROWSER_ENV */\nlet x = 1;'), []);
  assert.deepEqual(kinds('app/src/x.rs', 'let a = var("SPOTTER_CHROMIUM_PATH");\nlet b = var("CHROMIUM_TEST_PATH_HINT");\nlet c = MY_TEST_BROWSER_ENVY;'), []);
});

test('codeWithStrings keeps literals and drops comments, offsets unchanged', () => {
  const src = 'let a = "x // y"; // "CHROMIUM_TEST_PATH"\nlet r = r#"q"#;';
  const out = codeWithStrings(src);
  assert.equal(out.length, src.length);
  assert.ok(out.includes('"x // y"'));
  assert.ok(!out.includes('CHROMIUM_TEST_PATH'));
});

test('C55 through the runner: one error per ungated test or direct read', async () => {
  const root = makeApp({
    'crates/r/tests/real_browser.rs': 'fn b() -> Option<B> { find_for_tests(Mode::Headless) }\n' + t('#[test]', '    let r = ChromiumRenderer::new(a, b().unwrap());'),
    'crates/r/tests/ok.rs': 'fn b() -> Option<B> { find_for_tests(Mode::OffScreen) }\n' + t('#[test]', '    let r = browser(Mode::OffScreen);\n    let r = ChromiumRenderer::new(a, b);'),
    'crates/r/src/discover.rs': 'pub const TEST_BROWSER_ENV: &str = "SPOTTER_TEST_BROWSER";\n',
    'app/src/live.rs': 'fn f() { let _ = std::env::var_os("CHROMIUM_TEST_PATH"); }\n',
  });
  const found = await findingsOf('C55', root);
  assert.deepEqual(found.map((f) => [f.file, f.severity]).sort(), [['app/src/live.rs', 'error'], ['crates/r/tests/real_browser.rs', 'error']]);
});
