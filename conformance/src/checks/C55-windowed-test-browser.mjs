// C55: tests never open a browser window on the user's desktop (rule 7; lesson "Live-browser tests opened Chrome on the
// user's desktop"). A test browser's window may open only on Linux under a virtual display (xvfb-run, with
// PADDOCK_LIVE_BROWSER=1 set by the CI job); the test-browser finders (`discover.rs`) carry that gate. Flagged:
// - in any Rust file but a `discover.rs`, reading `CHROMIUM_TEST_PATH` directly or calling `find_for_tests_with(`: both go
//   around the finder and its gate;
// - in a Rust integration test (`tests/`), a test fn that launches a headful browser (`Mode::OffScreen`, `Mode::OnScreen`,
//   or a `ChromiumRenderer::new(` whose default mode is headful) without `Mode::Headless` and without asking the gate
//   (`windowed_tests_here()`, or the driver's `test_browser()`).
import { error } from '../finding.mjs';
import { maskRust } from './C39-freed-port.mjs';

const DIRECT = /\bvar(?:_os)?\s*\(\s*"CHROMIUM_TEST_PATH"|\bfind_for_tests_with\s*\(/;
const HEADFUL = /\bMode::(?:OffScreen|OnScreen)\b|\bChromiumRenderer::new\s*\(/;
const GATE = /\bwindowed_tests_here\s*\(|\btest_browser\s*\(/;

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
  const out = [];
  const base = path.split('/').pop();
  if (base !== 'discover.rs') {
    text.split('\n').forEach((l, i) => {
      if (DIRECT.test(l) && !/^\s*(?:\/\/|\*)/.test(l)) out.push({ line: i + 1, kind: 'direct' });
    });
  }
  if (!/(^|\/)tests\//.test(path)) return out;
  const masked = maskRust(text);
  const lineOf = (i) => masked.slice(0, i).split('\n').length;
  for (const t of masked.matchAll(/#\[(?:[\w]+::)*test(?:\([^)]*\))?\]/g)) {
    const fn = /\bfn\s+(\w+)[^{;]*\{/.exec(masked.slice(t.index));
    if (!fn) continue;
    const open = t.index + fn.index + fn[0].length - 1;
    const end = closing(masked, open);
    if (end < 0) continue;
    const body = masked.slice(open, end);
    if (HEADFUL.test(body) && !/\bMode::Headless\b/.test(body) && !GATE.test(body)) {
      out.push({ line: lineOf(t.index + fn.index), kind: 'ungated', name: fn[1] });
    }
  }
  return out;
}

export default {
  id: 'C55',
  title: 'Tests open a browser window only under a virtual display',
  rule: 7,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      if (!f.endsWith('.rs')) continue;
      const text = ctx.read(f);
      if (!text || !/CHROMIUM_TEST_PATH|find_for_tests_with|Mode::|ChromiumRenderer/.test(text)) continue;
      for (const p of windowedTestFindings(f, text)) {
        if (p.kind === 'direct') {
          out.push(error('C55', 7, f, p.line, 'reads the test browser around its finder (and the finder\'s window gate)', 'Get the test browser from the crate\'s `discover::find_for_tests()` / `test_browser()`, which hand it out only where its window lands on a virtual display.'));
        } else {
          out.push(error('C55', 7, f, p.line, `test \`${p.name}\` opens a headful browser window without the gate: on a desktop runner it lands on the user's screen`, 'Launch it headless (`Mode::Headless`), or return early unless `discover::windowed_tests_here()` (Linux under xvfb-run with PADDOCK_LIVE_BROWSER=1).'));
        }
      }
    }
    return out;
  },
};
