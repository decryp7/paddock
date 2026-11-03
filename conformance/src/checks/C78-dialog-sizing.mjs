// C78 (ADR 0043, check 3): the size of a dialog window is decided in one place.
//  (a) No `set_size`, `set_min_size`, `set_max_size` or `set_position` on a dialog window outside `crates/paddock-dialogs`: in a Rust
//      file of the app that uses `paddock_dialogs` (production code; comments and `#[cfg(test)]` items are not read).
//  (b) No TypeScript of the app calls `dialog_measure` or `dialog_ready`: the shared `shell/dialogs/src/io.ts` is the only caller,
//      and it declares no wire type of its own (`DialogMeasureWire` and the rest are generated).
//  (c) A dialog content component of a decision, form or viewer sets no height that follows the window (a ratchet: with a viewer
//      that never shrinks it grows the window to the cap): no `vh`/`dvh`/`svh` unit, no `100%` height or min-height, no `flex: 1` or
//      `flex-grow`, in a file under a `dialogs/` folder of the app (a tool's file, named `*-tool.ts` or declaring `kind: 'tool'`, may).
import { error } from '../finding.mjs';
import { withoutTestItems } from '../util/rust-test-items.mjs';
import { maskRust } from './C39-freed-port.mjs';

const WINDOW_CALL = /\.\s*(set_size|set_min_size|set_max_size|set_position)\s*\(/g;
const TS_CALL = /['"`](dialog_measure|dialog_ready)['"`]/g;
/** A wire type declared by hand: `interface DialogMeasure`, `type DialogStart = {..}`; an alias of the generated `Dialog*Wire` or a re-export is fine. */
const OWN_WIRE_TYPE = /^\s*(?:export\s+)?(?:interface|type)\s+(Dialog\w*(?:Measure|Wire|Start|Info|Result|Shown|Closed|Error|Request|Remembered))\b(?!\s*=\s*Dialog\w*Wire\s*;)/gm;
const RATCHET = [
  [/\b\d+(?:\.\d+)?(?:vh|dvh|svh)\b/, 'a viewport-height unit'],
  [/(?:^|[;{\s])(?:min-)?height\s*:\s*100%/, 'a height of 100%'],
  [/\bflex\s*:\s*1\b/, 'flex: 1'],
  [/\bflex-grow\s*:\s*[1-9]/, 'flex-grow'],
];

/** The window-sizing calls in the production part of a Rust source: [{ line, name }]. */
export function windowSizingCalls(source) {
  const production = withoutTestItems(maskRust(source));
  return [...production.matchAll(WINDOW_CALL)].map((m) => ({ line: production.slice(0, m.index).split('\n').length, name: m[1] }));
}

/** The wire types a source declares itself: [{ line, name }]. */
export function ownWireTypes(source) {
  return [...source.matchAll(OWN_WIRE_TYPE)].map((m) => ({ line: source.slice(0, m.index).split('\n').length, name: m[1] }));
}

/** The ratchets in a dialog content source: [{ line, what }]. */
export function ratchets(source) {
  const out = [];
  source.split('\n').forEach((text, i) => {
    for (const [re, what] of RATCHET) if (re.test(text)) out.push({ line: i + 1, what });
  });
  return out;
}

export default {
  id: 'C78',
  title: 'A dialog window is sized only by paddock-dialogs, over dialog_measure and dialog_ready, and its content has no height that follows the window',
  rule: 40,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      const text = ctx.read(f);
      if (!text) continue;
      if (f.endsWith('.rs') && !f.includes('crates/paddock-dialogs/') && /paddock_dialogs/.test(text) && !/(^|\/)(?:tests?|benches)(?:\.rs$|\/)/.test(f)) {
        for (const c of windowSizingCalls(text)) {
          out.push(error('C78', 40, f, c.line, `${c.name}(..) in a file that uses paddock_dialogs: a dialog window is sized only by paddock-dialogs`, 'Do not size a dialog window here: the page measures, `paddock_dialogs::size` decides and `tauri/sizing.rs` carries it out (ADR 0043).'));
        }
      }
      if (f.endsWith('.ts') && !f.endsWith('.spec.ts') && !f.includes('shell/dialogs/src/io.ts')) {
        for (const m of text.matchAll(TS_CALL)) {
          out.push(error('C78', 40, f, text.slice(0, m.index).split('\n').length, `${m[1]} is called outside shell/dialogs/src/io.ts`, 'Size and show a dialog through the dialog frame: it measures and `DialogIo` sends the measure.'));
        }
      }
      if (f.endsWith('shell/dialogs/src/io.ts')) {
        for (const w of ownWireTypes(text)) {
          out.push(error('C78', 40, f, w.line, `io.ts declares the wire type ${w.name} itself`, 'Alias the generated type (`export type DialogMeasure = DialogMeasureWire`): the wire has one definition, in Rust.'));
        }
      }
      const isContent = /\/dialogs\/[^/]+\.ts$/.test(f) && !f.endsWith('.spec.ts');
      if (isContent && !/-tool\.ts$/.test(f) && !/kind:\s*'tool'/.test(text)) {
        for (const r of ratchets(text)) {
          out.push(error('C78', 40, f, r.line, `dialog content with ${r.what}: its height follows the window, so a window that grows with its content ratchets to the cap`, 'Let the content take its natural height; the frame measures it (a body minimum goes through dialogMinBodyHeight).'));
        }
      }
    }
    return out;
  },
};
