// C55: tests never start a real browser or app on a desktop someone uses (rule 7: only in a throwaway Linux CI container or
// a test VM; lesson "live-browser tests opened Chrome on the user's desktop").
//
// The structural gate is in the drivers, not here: in test builds each driver's launch (`paddock-chromium`, Debrief's
// `chromium-driver`, Spotter's `spotter-render`) asks `paddock_testkit::live_browser::check_launch` before it starts any
// program (no installed browser ever; no real program outside a throwaway session), and the test-browser finders
// (`src/discover.rs`) ask the same gate. This check is the backstop for code that goes around a driver, and reads every Rust
// file but a `src/discover.rs` (comments never count; a string literal does). Flagged:
// - anywhere: reading the test browser around its finder: the literals "CHROMIUM_TEST_PATH" or "<APP>_TEST_BROWSER[_DIR]"
//   (also split up in a `concat!`), the constant `TEST_BROWSER_ENV`, or a call of `find_for_tests_with(`;
// - in test code (a `tests/` folder, a `tests.rs` or `*_tests.rs` file, a `#[cfg(test)]` module, a test fn):
//   - the product's browser search: `discover::find(`, `chromium_driver::find(`, `find_installed(`, `find_downloaded(`,
//     `runtime::installed(`, or a `use` that imports a driver's `find`;
//   - a Spotter renderer without its browser: `ChromiumRenderer::new(..)` in a statement without `.with_browser(` (it
//     falls through to the installed browser);
//   - starting a browser or app by name: `Command::new("google-chrome" | "chrome" | "chromium" | "msedge" | "open" ..)`;
//   - the self-tests and the status that start a browser to ask its version: `renderer_status(`, `install::self_test(`,
//     `discover::candidates(`, and a test fn that both names installed candidates and calls `self_test(` (the drivers'
//     self-tests ask the launch gate in test builds; this names a test that would start the user's browser);
//   - the user's default browser: `open::that*(`, a call of `open_url(` (not a method of a fake: `f.open_url(`),
//     `Command::new("xdg-open")`, `Command::new("cmd")` with `start`;
//   - a hard-coded installed browser: a literal path into `/Applications/{Google Chrome,Chromium,Microsoft Edge}*.app`, `Google\Chrome\Application`,
//     `/usr/bin/google-chrome*` or `/usr/bin/chromium*` (a bare `chrome.exe` is a process name in many tests' data);
//   - in a file that gets a test browser (`find_for_tests(`, `test_browser(`), a test fn that launches a window
//     (`Mode::`/`Window::OffScreen`/`OnScreen`, or default options) after asking the finder for none (the gate then never
//     checked for the virtual display). Helper fns are not followed: the launch-time check refuses a real browser outside
//     a throwaway session whatever the helper did, so this rule only names the mismatch early.
// A deliberate case carries `// conformance: no-launch — <reason>` on the line or the line above, or anywhere in the test fn
// it is in.
import { error } from '../finding.mjs';
import { maskRust } from './C39-freed-port.mjs';

const DIRECT = /"(?:CHROMIUM_TEST_PATH|[A-Z][A-Z0-9]*_TEST_BROWSER(?:_DIR)?)"|\bTEST_BROWSER_ENV\b|\bfind_for_tests_with\s*\(|\bconcat!\s*\([^)]*"(?:CHROMIUM_?|[A-Z0-9_]*TEST_?BROWSER|[A-Z0-9_]*TEST_?PATH)/g;
const PRODUCT_SEARCH = /\b(?:discover|chromium_driver)::find\s*\(|\bfind_installed(?:_with)?\s*\(|\bfind_downloaded\s*\(|\bruntime::installed\s*\(|\buse\s+[\w:]*(?:discover|chromium_driver)::(?:\{[^}]*\bfind\b[^}]*\}|find\b)/g;
const COMMAND = /\bCommand::new\s*\(\s*"(?:[^"]*[\\/])?(?:google-chrome(?:-stable)?|chrome|chromium(?:-browser)?|msedge|microsoft-edge|Google Chrome|Microsoft Edge|Chromium|open|xdg-open)(?:\.exe)?"/g;
const INSTALLED_PATH = /"[^"\n]*(?:\/Applications\/(?:Google Chrome|Chromium|Microsoft Edge)[^"\n\/]*\.app|Google\\{1,2}Chrome\\{1,2}Application|\/usr\/bin\/(?:google-chrome|chromium))[^"\n]*"/g;
const SELF_TEST = /\brenderer_status\s*\(|\binstall::self_test\s*\(|\bdiscover::candidates\s*\(/g;
const BARE_SELF_TEST = /\bself_test\s*\(/;
const NAMES_INSTALLED = /(?<![.\w])(?:[\w:]*::)?candidates\s*\(|\bfind_installed/;
const OPENER = /\bopen::that\w*\s*\(|(?<![.\w])(?:[\w:]*::)?open_url\s*\(|\bCommand::new\s*\(\s*"cmd(?:\.exe)?"\s*\)[^;]*"start"/g;
const RENDERER = /\bChromiumRenderer::new\s*\(/g;
const FINDER = /\b(?:find_for_tests|test_browser)\s*\(/;
const HEADFUL = /\b(?:Mode|Window)::(?:OffScreen|OnScreen)\b|\bChromiumRenderer::new\s*\(|\bLaunchOptions::(?:new|default)\s*\(|\bLaunchOptions\s*\{|\bChromium::launch\s*\(/;
const HEADLESS = /headless/i;
const WINDOWED_FINDER = /\b(?:find_for_tests|browser)\s*\(\s*(?:\w+::)*(?:Mode|Window)::(?:OffScreen|OnScreen)\b|\btest_browser\s*\(/;
const WAIVER = /conformance:\s*no-launch\s*[—–-]+\s*\S.{2,}/;

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

/** The [start, end) ranges of test code in a file: all of it for a test file; else `#[cfg(..test..)]` modules and test fns. */
export function testRanges(path, masked) {
  if (/(^|\/)tests\/|(^|\/)(?:\w+_)?tests\.rs$/.test(path)) return [[0, masked.length]];
  const ranges = [];
  const block = (from) => {
    const head = /\b(?:mod|fn)\s+\w+[^{;]*\{/.exec(masked.slice(from));
    if (!head) return;
    const open = from + head.index + head[0].length - 1;
    const end = closing(masked, open);
    if (end > 0) ranges.push([from, end]);
  };
  for (const a of masked.matchAll(/#\[cfg\([^\]]*\btest\b[^\]]*\]\s*(?:#\[[^\]]*\]\s*)*mod\s+\w+\s*\{/g)) block(a.index);
  for (const a of masked.matchAll(/#\[(?:[\w]+::)*test(?:\([^)]*\))?\]/g)) block(a.index);
  return ranges;
}

/** The findings of one Rust file: [{ line, kind, name? }]. Kinds: direct, search, renderer, command, path, ungated. */
export function windowedTestFindings(path, text) {
  if (/(^|\/)src\/discover\.rs$/.test(path)) return [];
  const code = codeWithStrings(text);
  const masked = maskRust(text);
  const lines = text.split('\n');
  const lineOf = (i) => code.slice(0, i).split('\n').length;
  const ranges = testRanges(path, masked);
  // The test fns (not whole modules or files) a waiver anywhere inside covers.
  const fnRanges = [...masked.matchAll(/#\[(?:[\w]+::)*test(?:\([^)]*\))?\]/g)].map((t) => {
    const fn = /\bfn\s+\w+[^{;]*\{/.exec(masked.slice(t.index));
    const open = fn ? t.index + fn.index + fn[0].length - 1 : -1;
    return [t.index, open < 0 ? -1 : closing(masked, open)];
  });
  const waived = (line, index) =>
    WAIVER.test(lines[line - 1] ?? '') ||
    WAIVER.test(lines[line - 2] ?? '') ||
    fnRanges.some(([a, b]) => index >= a && index < b && WAIVER.test(text.slice(a, b)));
  const out = [];
  const push = (index, kind, name) => {
    const line = lineOf(index);
    if (!waived(line, index)) out.push({ line, kind, ...(name ? { name } : {}) });
  };
  for (const m of code.matchAll(DIRECT)) push(m.index, 'direct');
  const inTest = (i) => ranges.some(([a, b]) => i >= a && i < b);
  for (const [re, kind] of [[PRODUCT_SEARCH, 'search'], [COMMAND, 'command'], [INSTALLED_PATH, 'path'], [SELF_TEST, 'selftest'], [OPENER, 'opener']]) {
    for (const m of code.matchAll(re)) if (inTest(m.index)) push(m.index, kind);
  }
  // A test fn that self-tests what it took from the installed candidates.
  for (const [a, b] of fnRanges) {
    if (b < 0) continue;
    const body = masked.slice(a, b);
    if (NAMES_INSTALLED.test(body) && BARE_SELF_TEST.test(body)) push(a + body.search(BARE_SELF_TEST), 'selftest');
  }
  for (const m of masked.matchAll(RENDERER)) {
    if (!inTest(m.index)) continue;
    const end = masked.indexOf(';', m.index);
    const statement = masked.slice(m.index, end < 0 ? masked.length : end);
    if (!/\.with_browser\s*\(/.test(statement)) push(m.index, 'renderer');
  }
  if (FINDER.test(masked)) {
    for (const t of masked.matchAll(/#\[(?:[\w]+::)*test(?:\([^)]*\))?\]/g)) {
      const fn = /\bfn\s+(\w+)[^{;]*\{/.exec(masked.slice(t.index));
      if (!fn) continue;
      const open = t.index + fn.index + fn[0].length - 1;
      const end = closing(masked, open);
      if (end < 0) continue;
      const body = masked.slice(open, end);
      // What the finder was asked for is not how the browser is launched: a `Headless` in its call does not count.
      const launches = body.replace(/\b(?:find_for_tests|browser)\s*\([^)]*\)/g, '');
      if (HEADFUL.test(body) && !HEADLESS.test(launches) && !WINDOWED_FINDER.test(body)) push(t.index + fn.index, 'ungated', fn[1]);
    }
  }
  return out.sort((a, b) => a.line - b.line);
}

const MESSAGES = {
  direct: ['reads the test browser around its finder (and the real-browser gate)', 'Get the test browser from the driver\'s `discover::find_for_tests(mode)` (or Debrief\'s `test_browser()`).'],
  search: ['a test uses the product\'s browser search, which finds the browser the user installed', 'Use the pinned test browser from `discover::find_for_tests(mode)`; a test never starts an installed browser.'],
  renderer: ['a test builds a renderer without `.with_browser(..)`: it falls through to the installed browser', 'Give it the test browser: `.with_browser(find_for_tests(mode)?)`, or a fake.'],
  command: ['a test starts a browser or an app by name', 'Start the pinned test browser through the driver, only where the gate allows it; never `open` an app from a test.'],
  selftest: ['a test asks a browser for its version by starting it (a self-test or the renderer status), or lists the installed candidates', 'Self-test the pinned test browser or a fake; the drivers refuse an installed browser in test builds, and a test that checks that refusal says so with the waiver.'],
  opener: ['a test opens the user\'s default browser', 'Short-circuit the opener in test builds (`if cfg!(test) { return Ok(()) }`) and assert the call, never open a page from a test.'],
  path: ['a test names an installed browser\'s path', 'Use the pinned test browser from the driver\'s finder, never a path into the user\'s installed apps.'],
};

export default {
  id: 'C55',
  title: 'Tests start a real browser only in a throwaway session',
  rule: 7,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      if (!f.endsWith('.rs')) continue;
      const text = ctx.read(f);
      if (!text) continue;
      for (const p of windowedTestFindings(f, text)) {
        if (p.kind === 'ungated') {
          out.push(error('C55', 7, f, p.line, `test \`${p.name}\` launches a browser with a window but asked the finder for none: the gate never checked for the virtual display`, 'Launch it headless (`Mode::Headless` / `Window::Headless`), or ask the finder for the window it opens (`find_for_tests(Mode::OffScreen)`).'));
        } else {
          const [message, fix] = MESSAGES[p.kind];
          out.push(error('C55', 7, f, p.line, message, `${fix} A deliberate case carries \`// conformance: no-launch — <reason>\`.`));
        }
      }
    }
    return out;
  },
};
