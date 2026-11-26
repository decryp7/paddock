/**
 * Whether a `cfg(...)` predicate (the text inside the parentheses) marks test-only code: `test`, `all(..)` with a test-only part
 * (`all(unix, test)`), or `any(..)` with one (`any(test, feature = "testing")`: test support by convention, as C128 read it before).
 * `not(..)` never does. The Rust twin is `without_test_items` in tools/paddock-dev/src/check/lex/rust.rs; the two run the same
 * cases (tools/paddock-dev/tests/fixtures/test-items-corpus.tsv).
 */
export function impliesTest(predicate) {
  const p = predicate.trim();
  const m = /^(all|any|not)\s*\(([\s\S]*)\)$/.exec(p);
  if (!m) return p === 'test';
  if (m[1] === 'not') return false;
  let depth = 0;
  let from = 0;
  const parts = [];
  for (let i = 0; i < m[2].length; i++) {
    const c = m[2][i];
    if (c === '(') depth++;
    else if (c === ')') depth--;
    else if (c === ',' && depth === 0) {
      parts.push(m[2].slice(from, i));
      from = i + 1;
    }
  }
  parts.push(m[2].slice(from));
  return parts.some(impliesTest);
}

/** [start, end) of each test-only `#[cfg(..)]` attribute of `masked`: the attribute alone, `end` just past its `]`. */
function testAttributes(masked, predicate = impliesTest) {
  const found = [];
  for (const m of masked.matchAll(/#\[\s*cfg\s*\(/g)) {
    let depth = 1;
    let i = m.index + m[0].length;
    const inner = i;
    for (; i < masked.length && depth > 0; i++) depth += masked[i] === '(' ? 1 : masked[i] === ')' ? -1 : 0;
    const close = /^\s*\]/.exec(masked.slice(i));
    if (depth === 0 && close && predicate(masked.slice(inner, i - 1))) found.push([m.index, i + close[0].length]);
  }
  return found;
}

/**
 * `masked` (a `maskRust` result) with every test-only item (`#[cfg(test)]`, `#[cfg(all(unix, test))]`, see `impliesTest`) blanked, newlines and offsets kept: a `mod name { … }` or fn
 * with a body (brace-matched) or a single item ending in `;` (a `use`, a `const`). Code after a test item stays visible, so a
 * leading `#[cfg(test)] use …;` hides nothing below it. `predicate` decides which `cfg` predicates mark an item to blank (default
 * `impliesTest`; a check of another cfg, such as C171's `unix`, passes its own).
 */
export function withoutTestItems(masked, predicate = impliesTest) {
  const chars = [...masked];
  const blank = (from, to) => {
    for (let i = from; i < to; i++) if (chars[i] !== '\n') chars[i] = ' ';
  };
  for (const [attrStart, attrEnd] of testAttributes(masked, predicate)) {
    let i = attrEnd;
    // Further attributes on the same item.
    for (;;) {
      const next = /^\s*#\[[^\]]*\]/.exec(masked.slice(i));
      if (!next) break;
      i += next[0].length;
    }
    let depth = 0;
    let end = masked.length;
    for (; i < masked.length; i++) {
      const c = masked[i];
      if (c === '(' || c === '[') depth++;
      else if (c === ')' || c === ']') depth--;
      else if (depth <= 0 && c === ';') {
        end = i + 1;
        break;
      } else if (depth <= 0 && c === '{') {
        let braces = 1;
        for (i++; i < masked.length && braces > 0; i++) braces += masked[i] === '{' ? 1 : masked[i] === '}' ? -1 : 0;
        end = i;
        break;
      }
    }
    blank(attrStart, end);
  }
  return chars.join('');
}

/** `masked` with everything but the test-only items blanked (newlines and offsets kept): what an audit of test code reads. */
export function onlyTestItems(masked) {
  const production = withoutTestItems(masked);
  const chars = [...masked];
  for (let i = 0; i < chars.length; i++) if (chars[i] !== '\n' && production[i] === chars[i]) chars[i] = ' ';
  return chars.join('');
}
