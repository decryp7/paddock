// C62: every limit is in a registry and every registry row has a test (ADR 0019 check 2, plan 1.4b; Debrief ADR 0065 part 3).
// Two directions, both at warning level until the Debrief list is reasoned (ADR 0019 phases it so):
//  1. a row declared in a crate (`LimitRow::stop("id", ...)` or `id: LimitId::new("id")`) has a case in that crate, a
//     `limit_cases!` entry (`name: "id" => stop(...)`) or a `limit_case!(stop|cut, REGISTRY, "id", ...)` call;
//  2. in a crate that has a registry, a limit-shaped constant (`MAX_*`, `*_LIMIT`, `*_TIMEOUT`, `*_CAP`, `*_BUDGET`) that no
//     registry file mentions is reported once per crate with its first names. Input-validation lengths are waived by name in
//     `paddock.conformance.json` ("limitWaivers": ["MAX_NAME", ...]) or whole crates ("limitSkip": ["crate"]).
// A row is found two ways: the literal forms above, and any `const NAME: LimitRow = ...;` item, whose id is the first dotted string
// literal in it (a crate's own helper such as `row("archive.entries", ...)` or `hard("view.nodes", ...)` is no `LimitRow::stop`).
// Rows in `#[cfg(test)]` modules, in tests/ and in tests.rs are fixtures and are not checked. The tests themselves run in Rust (`limit_cases!` compares the cases with the registry's rows both ways); this check finds a
// missing case without compiling, and the unregistered constants that no test can find.
import { warn } from '../finding.mjs';

const ROW_ID = /\bLimitRow::stop\s*\(\s*"([a-z][a-z0-9_.]*)"|\bid\s*:\s*LimitId::new\s*\(\s*"([a-z][a-z0-9_.]*)"/g;
const CASE_ID = /"([a-z][a-z0-9_.]*)"\s*=>\s*(?:stop|cut)\s*\(|\blimit_case!\s*\(\s*(?:stop|cut)\s*,[^"]*"([a-z][a-z0-9_.]*)"/g;
const LIMIT_CONST = /\bconst\s+((?:MAX_[A-Z0-9_]+|[A-Z0-9_]+_(?:LIMIT|TIMEOUT|CAP|BUDGET)))\s*:/g;

const ROW_ITEM = /\bconst\s+[A-Z0-9_]+\s*:\s*LimitRow\s*=/g;
const ID_LITERAL = /"([a-z][a-z0-9_]*\.[a-z0-9_.]*[a-z0-9_])"/;

/** The end of the item that starts at `from`: the first `;` outside brackets, or the matching `}` of a block (strings skipped). */
function itemEnd(source, from) {
  let depth = 0;
  for (let i = from; i < source.length; i += 1) {
    const c = source[i];
    if (c === '"') {
      for (i += 1; i < source.length && source[i] !== '"'; i += 1) if (source[i] === '\\') i += 1;
    } else if (c === '(' || c === '[' || c === '{') depth += 1;
    else if (c === ')' || c === ']') depth -= 1;
    else if (c === '}') {
      depth -= 1;
      if (depth <= 0) return i + 1;
    } else if (c === ';' && depth <= 0) return i + 1;
  }
  return source.length;
}

/** The source without its `#[cfg(test)]` items (a module, a function or a use), keeping the line count. */
export function withoutTestItems(source) {
  let out = '';
  let at = 0;
  for (const m of source.matchAll(/#\[cfg\(test\)\]/g)) {
    if (m.index < at) continue;
    const end = itemEnd(source, m.index + m[0].length);
    out += source.slice(at, m.index) + source.slice(m.index, end).replace(/[^\n]/g, ' ');
    at = end;
  }
  return out + source.slice(at);
}

/** Ids of the limit rows declared in a Rust source: [{ id, line }]. */
export function rowIds(source) {
  const lineAt = (i) => source.slice(0, i).split('\n').length;
  const found = new Map();
  for (const m of source.matchAll(ROW_ID)) found.set(m.index, { id: m[1] ?? m[2], line: lineAt(m.index) });
  for (const m of source.matchAll(ROW_ITEM)) {
    const from = m.index + m[0].length;
    const lit = ID_LITERAL.exec(source.slice(from, itemEnd(source, from)));
    if (lit && ![...found.values()].some((r) => r.id === lit[1])) found.set(from + lit.index, { id: lit[1], line: lineAt(from + lit.index) });
  }
  return [...found.entries()].sort((a, b) => a[0] - b[0]).map(([, r]) => r);
}

/** Ids that a source has a case for. */
export function caseIds(source) {
  return [...source.matchAll(CASE_ID)].map((m) => m[1] ?? m[2]);
}

/** Names of the limit-shaped constants of a source (test modules are not skipped: a constant there is no limit either way). */
export function limitConstants(source) {
  return [...source.matchAll(LIMIT_CONST)].map((m) => m[1]);
}

const crateOf = (file) => /^(.*?\/?(?:crates\/)?[^/]+)\/(?:src|tests)\//.exec(file)?.[1] ?? null;

export default {
  id: 'C62',
  title: 'Every limit row has a case, and limit-shaped constants are in a registry',
  rule: 40,
  run(ctx) {
    const skip = new Set(ctx.config.limitSkip ?? []);
    const waived = new Set(ctx.config.limitWaivers ?? []);
    const byCrate = new Map();
    for (const f of ctx.files().filter((x) => x.endsWith('.rs') && !/(^|\/)(fuzz|target)\//.test(x))) {
      const crate = crateOf(f);
      if (!crate || skip.has(crate.split('/').pop())) continue;
      if (!byCrate.has(crate)) byCrate.set(crate, []);
      byCrate.get(crate).push(f);
    }
    const out = [];
    for (const [crate, files] of byCrate) {
      const rows = [];
      const cases = new Set();
      const registryText = [];
      const constants = [];
      for (const f of files) {
        const src = ctx.read(f) ?? '';
        // A row declared in a `#[cfg(test)]` module or under tests/ is a fixture, not a limit.
        const found = /(^|\/)(tests\/|tests\.rs$)/.test(f) ? [] : rowIds(withoutTestItems(src));
        for (const r of found) rows.push({ ...r, file: f });
        for (const id of caseIds(src)) cases.add(id);
        if (found.length > 0) registryText.push(src);
        for (const name of limitConstants(src)) constants.push({ name, file: f });
      }
      for (const r of rows) {
        if (!cases.has(r.id)) {
          out.push(warn('C62', 40, r.file, r.line, `limit row "${r.id}" has no limit_cases! case in ${crate}`, 'Add `name: "' + r.id + '" => stop(got = ..., limit = ..., setting = ...)` (or `=> cut(...)`) to the crate\'s limit_cases! with the data at and over the limit.'));
        }
      }
      if (rows.length === 0) continue;
      const mentioned = registryText.join('\n');
      const loose = [...new Set(constants.filter((c) => !waived.has(c.name) && !new RegExp(`\\b${c.name}\\b`).test(mentioned)).map((c) => c.name))];
      if (loose.length > 0) {
        const at = constants.find((c) => c.name === loose[0]);
        out.push(warn('C62', 40, at.file, 1, `${crate} has ${loose.length} limit-shaped constant(s) no registry file mentions (first: ${loose.slice(0, 5).join(', ')})`, 'Make each a row of the crate\'s registry with a case, or list the input-validation ones in "limitWaivers" of paddock.conformance.json.'));
      }
    }
    return out;
  },
};
