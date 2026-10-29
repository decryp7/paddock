// C52: Pacenotes never turns on `paddock-repo-graph`'s `cargo-metadata` feature (ADR 0022 §3.7, plan 2026-10-29 P0.1b).
// The feature runs `cargo metadata`, and cargo reads the analysed repository's `.cargo/config.toml` (compiler and wrapper
// programs); Pacenotes reads other people's repositories and must never run their code (AGENTS.md "Never execute
// repository code").
//
// This is the BACKSTOP. The real guard is Pacenotes' own CI leg: `cargo tree -e features --all-features -i
// paddock-repo-graph` (apps/pacenotes/scripts/check-no-cargo-metadata.mjs), which sees the resolved graph. This check
// reads the manifests under apps/pacenotes/ with a small TOML reader (comments and strings handled, arrays and inline
// tables across lines, quoted and dotted keys) and flags: the feature in the `features` of a dependency on the crate
// (inline table, `[dependencies.<name>]` table, workspace dependency, renamed with `package = ...`), and a feature of the
// manifest that forwards it as `<name>/cargo-metadata` or `<name>?/cargo-metadata`, <name> being the crate or any of its
// aliases. Deviation from ADR 0022 §3.7, which says to extend the WP-C6 check: C52 stands alone because WP-C6's
// check is a different mechanism (copies of shared CI scripts) and shares no code with this.
import { error } from '../finding.mjs';

const APP = 'apps/pacenotes/';
const FEATURE = 'cargo-metadata';
const CRATE = 'paddock-repo-graph';

/** Removes a `#` comment from a line, ignoring `#` inside basic ("...") and literal ('...') strings. */
export function stripComment(line) {
  let quote = null;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (quote === '"' && c === '\\') i++;
    else if (quote) { if (c === quote) quote = null; }
    else if (c === '"' || c === "'") quote = c;
    else if (c === '#') return line.slice(0, i);
  }
  return line;
}

/** Net depth change of `[`/`{` against `]`/`}` in a text, outside strings. */
function depth(text) {
  let d = 0;
  let quote = null;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quote === '"' && c === '\\') i++;
    else if (quote) { if (c === quote) quote = null; }
    else if (c === '"' || c === "'") quote = c;
    else if (c === '[' || c === '{') d++;
    else if (c === ']' || c === '}') d--;
  }
  return d;
}

/** Splits a dotted key (`a."b.c".d`) into its parts. */
export function splitKey(text) {
  const parts = [];
  let cur = '';
  let quote = null;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quote) { if (c === quote) quote = null; else cur += c; }
    else if (c === '"' || c === "'") quote = c;
    else if (c === '.') { parts.push(cur.trim()); cur = ''; }
    else cur += c;
  }
  parts.push(cur.trim());
  return parts;
}

/** Index of the first `=` outside strings, or -1. */
function equalsAt(line) {
  let quote = null;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (quote) { if (c === quote) quote = null; }
    else if (c === '"' || c === "'") quote = c;
    else if (c === '=') return i;
  }
  return -1;
}

/**
 * The key/value entries of a manifest: `[{ table: [...], key: [...], value: '<raw text>', line }]`. Values that span lines
 * (arrays, inline tables) are joined. Multi-line strings are not supported (manifests do not need them here).
 */
export function entries(text) {
  const out = [];
  let table = [];
  const lines = text.split('\n');
  for (let i = 0; i < lines.length; i++) {
    const line = stripComment(lines[i]).trim();
    if (!line) continue;
    const eq = equalsAt(line);
    // A header starts with `[` and has no `=` outside quotes (`[target.'cfg(target_os = "windows")'.dependencies]` has one inside).
    const header = line.startsWith('[') && eq < 0 ? /^\[\[?(.+?)\]\]?$/.exec(line) : null;
    if (header) { table = splitKey(header[1]); continue; }
    if (eq < 0) continue;
    const start = i;
    let value = line.slice(eq + 1).trim();
    while (depth(value) > 0 && i + 1 < lines.length) value += ` ${stripComment(lines[++i]).trim()}`;
    out.push({ table, key: splitKey(line.slice(0, eq)), value, line: start + 1 });
  }
  return out;
}

const stringsIn = (raw) => [...raw.matchAll(/"((?:[^"\\]|\\.)*)"|'([^']*)'/g)].map((m) => m[1] ?? m[2]);
const DEP_TABLE = /^(?:dev-|build-)?dependencies$/;

/** Where a dependency table's entries live: the position of `dependencies` in a table path (also under a target or workspace table), or -1. */
function depsAt(table) {
  for (let i = 0; i < table.length; i++) if (DEP_TABLE.test(table[i])) return i;
  return -1;
}

/** Manifest lines (1-based) that switch the feature on. */
export function enablingLines(text) {
  const all = entries(text);
  // dependency name -> { name: real crate name, features: [strings], line }
  const deps = new Map();
  const touch = (alias, line) => {
    if (!deps.has(alias)) deps.set(alias, { crate: alias, features: [], line });
    return deps.get(alias);
  };
  for (const e of all) {
    const at = depsAt(e.table);
    if (at >= 0 && e.key.length >= 1 && e.table.length === at + 1) {
      // `[dependencies]` + `alias = ...` or `alias.features = [...]` / `alias.package = "..."`
      const alias = e.key[0];
      const d = touch(alias, e.line);
      const before = d.features.length;
      if (e.key.length === 1) {
        const pkg = /\bpackage\s*=\s*("([^"]*)"|'([^']*)')/.exec(e.value);
        if (pkg) d.crate = pkg[2] ?? pkg[3];
        const feats = /\bfeatures\s*=\s*(\[[^\]]*\])/.exec(e.value);
        if (feats) d.features.push(...stringsIn(feats[1]));
      } else if (e.key[1] === 'features') d.features.push(...stringsIn(e.value));
      else if (e.key[1] === 'package') d.crate = stringsIn(e.value)[0] ?? alias;
      if (d.features.length > before && d.features.includes(FEATURE)) d.hit = e.line;
    } else if (at >= 0 && e.table.length === at + 2) {
      // `[dependencies.alias]` + `features = [...]` / `package = "..."`
      const d = touch(e.table[at + 1], e.line);
      if (e.key[0] === 'features') {
        d.features.push(...stringsIn(e.value));
        if (stringsIn(e.value).includes(FEATURE)) d.hit = e.line;
      } else if (e.key[0] === 'package') d.crate = stringsIn(e.value)[0] ?? d.crate;
    }
  }
  const aliases = new Set([CRATE]);
  const hits = [];
  for (const [alias, d] of deps) {
    if (d.crate !== CRATE) continue;
    aliases.add(alias);
    if (d.features.includes(FEATURE)) hits.push(d.hit ?? d.line);
  }
  for (const e of all) {
    if (e.table.length !== 1 || e.table[0] !== 'features') continue;
    for (const s of stringsIn(e.value)) {
      const m = /^([^/?]+)\??\/(.+)$/.exec(s);
      if (m && m[2] === FEATURE && aliases.has(m[1])) hits.push(e.line);
    }
  }
  return [...new Set(hits)].sort((a, b) => a - b);
}

export default {
  id: 'C52',
  title: "Pacenotes does not enable paddock-repo-graph's cargo-metadata feature",
  rule: 40,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      if (!f.startsWith(APP) || !f.endsWith('Cargo.toml')) continue;
      for (const line of enablingLines(ctx.read(f) ?? '')) {
        out.push(error('C52', 40, f, line, `${f} enables ${CRATE}'s ${FEATURE} feature`, 'Remove the feature: it runs `cargo metadata` on repositories Pacenotes only reads. Read Cargo manifests with the crate\'s own parser (ADR 0022 §3.7).'));
      }
    }
    return out;
  },
};
