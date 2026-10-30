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
    'crates/r/tests/real_browser.rs': 'fn b() -> Option<B> { find_for_tests(Mode::Headless) }\n' + t('#[test]', '    let r = ChromiumRenderer::new(a, c).with_browser(b().unwrap());'),
    'crates/r/tests/ok.rs': 'fn b() -> Option<B> { find_for_tests(Mode::OffScreen) }\n' + t('#[test]', '    let r = browser(Mode::OffScreen);\n    let r = ChromiumRenderer::new(a, c).with_browser(r);'),
    'crates/r/src/discover.rs': 'pub const TEST_BROWSER_ENV: &str = "SPOTTER_TEST_BROWSER";\n',
    'app/src/live.rs': 'fn f() { let _ = std::env::var_os("CHROMIUM_TEST_PATH"); }\n',
  });
  const found = await findingsOf('C55', root);
  assert.deepEqual(found.map((f) => [f.file, f.severity]).sort(), [['app/src/live.rs', 'error'], ['crates/r/tests/real_browser.rs', 'error']]);
});

// Review P1-1: the bypasses the pre-filter let through. Each is caught in test code, and none in product code.
const inTestMod = (body) => `pub fn product() {}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #[test]\n    fn it() {\n${body}\n    }\n}\n`;
const PROBES = [
  ['search', '        let b = chromium_driver::find().unwrap();\n        Chromium::launch(&b, &LaunchOptions::default()).unwrap();'],
  ['search', '        let b = paddock_chromium::discover::find(None, None).unwrap();'],
  ['search', '        let b = discover::find_installed().unwrap();'],
  ['search', '        let b = discover::find_downloaded(&dir);'],
  ['search', '        let b = runtime::installed(&store).unwrap();'],
  ['renderer', '        let page = ChromiumRenderer::new(d.path(), d.path()).render("http://127.0.0.1:1/", None, t).await;'],
  ['renderer', '        let r = ChromiumRenderer::new(\n            d.path(),\n            d.path(),\n        );'],
  ['path', '        let b = Browser::chromium("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome");'],
  ['path', '        let b = Browser::chromium("C:\\\\Program Files\\\\Google\\\\Chrome\\\\Application\\\\chrome.exe");'],
  ['path', '        let b = Browser::chromium("/usr/bin/google-chrome-stable");'],
  ['command', '        std::process::Command::new("google-chrome").arg("--headless").spawn().unwrap();'],
  ['command', '        Command::new("open").args(["-a", "Debrief"]).status().unwrap();'],
  [['command', 'path'], '        Command::new("/usr/bin/chromium").spawn().unwrap();'],
  ['direct', '        let p = std::env::var(concat!("CHROMIUM_", "TEST_PATH")).unwrap();'],
];

test('C55 bad: every bypass probe of the review is caught in test code (src cfg(test) module and tests/)', () => {
  for (const [kind, body] of PROBES) {
    const want = [kind].flat();
    assert.deepEqual(kinds('crates/x/src/lib.rs', inTestMod(body)), want, body);
    assert.deepEqual(kinds('apps/y/crates/z/tests/it/probe.rs', `#[test]\nfn it() {\n${body}\n}\n`), want, body);
    assert.deepEqual(kinds('crates/x/src/launch/tests.rs', `fn helper() {\n${body}\n}\n`), want, `${body} (a tests.rs helper)`);
  }
  assert.deepEqual(kinds('crates/x/tests/it/a.rs', 'use chromium_driver::find;\n#[test]\nfn it() { let b = find(); }\n'), ['search']);
  assert.deepEqual(kinds('crates/x/tests/it/a.rs', 'use paddock_chromium::discover::{find, Browser};\n'), ['search']);
});

test('C55 good: the product\'s own search and launch outside test code, a renderer with its browser, a waiver with a reason', () => {
  const product = 'pub fn resolve() -> Option<Browser> {\n    discover::find_installed().or_else(|| runtime::installed(&s))\n}\npub fn r() -> ChromiumRenderer { ChromiumRenderer::new(a, b) }\npub fn spawn() { Command::new("open").spawn(); }\n';
  assert.deepEqual(kinds('crates/x/src/adapter.rs', product + inTestMod('        let x = 1;')), []);
  assert.deepEqual(kinds('crates/x/src/adapter.rs', inTestMod('        let r = ChromiumRenderer::new(a, b)\n            .with_browser(fake.browser());')), []);
  const waived = inTestMod('        // conformance: no-launch — only resolves; the launch gate refuses an installed browser\n        let r = ChromiumRenderer::new(a, b);');
  assert.deepEqual(kinds('crates/x/src/adapter.rs', waived), []);
  const bare = inTestMod('        // conformance: no-launch\n        let r = ChromiumRenderer::new(a, b);');
  assert.deepEqual(kinds('crates/x/src/adapter.rs', bare), ['renderer'], 'a waiver needs its reason');
  // Process names in data are not paths to an installed browser.
  assert.deepEqual(kinds('crates/x/tests/it/a.rs', '#[test]\nfn it() { let p = proc(10, "chrome.exe"); let q = "/Applications/Utilities/Terminal.app"; }\n'), []);
});
