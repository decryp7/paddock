// C45: a dialog contract names a command only through its `dialog_callable::<name>` marker (ADR 0026 §3), never by a
// string. `DialogCallable::__command(..)` and `__prefix(..)` are the constructors the markers and the `command_table!`
// macros expand to; they are `#[doc(hidden)]` public because a macro has to reach them, so only the compiler stops an
// ordinary call. Flagged, in any Rust file outside `crates/paddock-commands` (their home): a `__command(` or `__prefix(`
// call in production code. Not flagged: comments, doc comments and doctests (blanked by the mask), strings, integration
// tests (`tests/`, `tests.rs`), benches, and every `#[cfg(test)]` item (a module or a single `use`; code after it still counts).
import { error } from '../finding.mjs';
import { withoutTestItems } from '../util/rust-test-items.mjs';
import { maskRust } from './C39-freed-port.mjs';

const CALL = /\b__(?:command|prefix)\s*\(/g;
const HOME = 'crates/paddock-commands/';
const TEST_FILE = /(^|\/)(?:tests?|benches)(?:\.rs$|\/)/;

/** The calls to the hidden constructors in the production part of a Rust source: [{ line, name }]. */
export function hiddenConstructorCalls(source) {
  const clean = maskRust(source);
  const production = withoutTestItems(clean);
  return [...production.matchAll(CALL)].map((m) => ({
    line: production.slice(0, m.index).split('\n').length,
    name: m[0].replace(/\s*\($/, ''),
  }));
}

export default {
  id: 'C45',
  title: 'Dialog contracts use the dialog_callable markers, never the hidden constructors',
  rule: 40,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      if (!f.endsWith('.rs') || f.startsWith(HOME) || f.includes(`/${HOME}`) || TEST_FILE.test(f)) continue;
      const text = ctx.read(f);
      if (!text || !/__(?:command|prefix)\s*\(/.test(text)) continue;
      for (const c of hiddenConstructorCalls(text)) {
        out.push(error('C45', 40, f, c.line, `${c.name}(..) is called outside paddock-commands`, 'Name the command by its marker: `commands::dialog_callable::<name>` (declared with `#[dialog_callable]` on the command), `prefix(..)` or `plugin(..)` from `paddock_commands`.'));
      }
    }
    return out;
  },
};
