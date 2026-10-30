// C55: tests never start a browser on the user's desktop (rule 7; lesson "live-browser tests opened Chrome on the user's
// desktop"). A real test browser starts only in a throwaway Linux CI session (PADDOCK_LIVE_BROWSER=1, under xvfb-run for a
// window): the test-browser finders in each driver's `src/discover.rs` ask the shared gate (`paddock_testkit::live_browser`)
// before they hand one out, and take the window mode the test will launch with. This check is the backstop. Flagged, in
// any Rust file but a `src/discover.rs` (comments never count; a string literal does):
// - reading the test browser around the finder: the literals "CHROMIUM_TEST_PATH" or "<APP>_TEST_BROWSER[_DIR]", the
//   constant `TEST_BROWSER_ENV`, or a call of `find_for_tests_with(`;
// - in a file that gets a test browser (`find_for_tests(`, `test_browser(`), a test fn that launches a browser with a
//   window (`Mode::`/`Window::OffScreen`/`OnScreen`, or default options: `ChromiumRenderer::new(`, `LaunchOptions::new(`,
//   `LaunchOptions::default(`, `LaunchOptions {`, `Chromium::launch(`) and names no headless mode, but did not ask the
//   finder for a window (`find_for_tests(Mode::OffScreen)`, `browser(Window::OnScreen(..))`, Debrief's `test_browser(`):
//   the gate then never checked for the virtual display.
import { error } from '../finding.mjs';
import { maskRust } from './C39-freed-port.mjs';

const DIRECT = /"(?:CHROMIUM_TEST_PATH|[A-Z][A-Z0-9]*_TEST_BROWSER(?:_DIR)?)"|\bTEST_BROWSER_ENV\b|\bfind_for_tests_with\s*\(/g;
const FINDER = /\b(?:find_for_tests|test_browser)\s*\(/;
const HEADFUL = /\b(?:Mode|Window)::(?:OffScreen|OnScreen)\b|\bChromiumRenderer::new\s*\(|\bLaunchOptions::(?:new|default)\s*\(|\bLaunchOptions\s*\{|\bChromium::launch\s*\(/;
const HEADLESS = /headless/i;
const WINDOWED_FINDER = /\b(?:find_for_tests|browser)\s*\(\s*(?:\w+::)*(?:Mode|Window)::(?:OffScreen|OnScreen)\b|\btest_browser\s*\(/;

/** `text` without comments (blanked, offsets kept) but with the contents of string literals, so literals can be matched. */
export function codeWithStrings(text) {
  const m = maskRust(text);
  let out = '';
  let inString = false;
  for (let i = 0; i < m.length; i++) {
    if (m[i] === '"') {
      inString = !inString;
      out += '"';
    } else out += inString ? text[i] : m[i];
  }
  return out;
}

/** Index of the brace closing the one at `open` (in masked text), or -1. */
function closing(text, open) {
  let depth = 0;
  for (let i = open; i < text.length; i++) {
    if (text[i] === '{') depth++;
    else if (text[i] === '}' && --depth === 0) return i;
  }
  return -1;
}

/** The findings of one Rust file: [{ line, kind: 'direct' | 'ungated', name? }]. */
export function windowedTestFindings(path, text) {
  if (/(^|\/)src\/discover\.rs$/.test(path)) return [];
  const out = [];
  const code = codeWithStrings(text);
  const masked = maskRust(text);
  const lineOf = (i) => code.slice(0, i).split('\n').length;
  for (const m of code.matchAll(DIRECT)) out.push({ line: lineOf(m.index), kind: 'direct' });
  if (!FINDER.test(masked)) return out;
  for (const t of masked.matchAll(/#\[(?:[\w]+::)*test(?:\([^)]*\))?\]/g)) {
    const fn = /\bfn\s+(\w+)[^{;]*\{/.exec(masked.slice(t.index));
    if (!fn) continue;
    const open = t.index + fn.index + fn[0].length - 1;
    const end = closing(masked, open);
    if (end < 0) continue;
    const body = masked.slice(open, end);
    // What the finder was asked for is not how the browser is launched: a `Headless` in its call does not count.
    const launches = body.replace(/\b(?:find_for_tests|browser)\s*\([^)]*\)/g, '');
    if (HEADFUL.test(body) && !HEADLESS.test(launches) && !WINDOWED_FINDER.test(body)) {
      out.push({ line: lineOf(t.index + fn.index), kind: 'ungated', name: fn[1] });
    }
  }
  return out;
}

export default {
  id: 'C55',
  title: 'Tests start a real browser only in a throwaway Linux session',
  rule: 7,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      if (!f.endsWith('.rs')) continue;
      const text = ctx.read(f);
      if (!text || !/TEST_BROWSER|CHROMIUM_TEST_PATH|find_for_tests|test_browser\s*\(/.test(text)) continue;
      for (const p of windowedTestFindings(f, text)) {
        if (p.kind === 'direct') {
          out.push(error('C55', 7, f, p.line, 'reads the test browser around its finder (and the real-browser gate)', 'Get the test browser from the driver\'s `discover::find_for_tests(mode)` (or Debrief\'s `test_browser()`), which hands it out only in a throwaway Linux CI session.'));
        } else {
          out.push(error('C55', 7, f, p.line, `test \`${p.name}\` launches a browser with a window but asked the finder for none: the gate never checked for the virtual display`, 'Launch it headless (`Mode::Headless` / `Window::Headless`), or ask the finder for the window it opens (`find_for_tests(Mode::OffScreen)`).'));
        }
      }
    }
    return out;
  },
};
