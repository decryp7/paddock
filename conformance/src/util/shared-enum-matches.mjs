// The `match` expressions on a shared enum in Rust source, and which of them end in a catch-all arm. A catch-all (`_ =>` or a bare
// binding) on a shared enum means a variant added later (Dyno WP-8's `Interrupted`) compiles everywhere and is handled by whatever
// the default does. Without one, the compiler names every app's match that must change in the same change (rule 39).
import { maskRust } from '../checks/C39-freed-port.mjs';
import { rustTestRanges } from '../checks/C71-test-timing.mjs';

const KEYWORDS = new Set(['ref', 'mut', 'box']);

/** Index of the bracket that closes the one at `open`, or -1. */
function closing(text, open) {
  const pairs = { '(': ')', '[': ']', '{': '}' };
  let depth = 0;
  for (let k = open; k < text.length; k++) {
    if ('([{'.includes(text[k])) depth++;
    else if (')]}'.includes(text[k]) && --depth === 0) return pairs[text[open]] === text[k] ? k : -1;
  }
  return -1;
}

/** The arms of the match body `text[open..close]`: [{ pattern, at }] with `at` the offset of the pattern's first character. */
function arms(text, open, close) {
  const out = [];
  let k = open + 1;
  while (k < close) {
    while (k < close && /\s/.test(text[k])) k++;
    if (k >= close) break;
    const at = k;
    let depth = 0;
    let arrow = -1;
    for (; k < close; k++) {
      const c = text[k];
      if ('([{'.includes(c)) depth++;
      else if (')]}'.includes(c)) depth--;
      else if (depth === 0 && c === '=' && text[k + 1] === '>') {
        arrow = k;
        break;
      }
    }
    if (arrow < 0) break;
    out.push({ pattern: text.slice(at, arrow), at });
    k = arrow + 2;
    while (k < close && /\s/.test(text[k])) k++;
    if (text[k] === '{') {
      const e = closing(text, k);
      k = e < 0 ? close : e + 1;
      while (k < close && /\s/.test(text[k])) k++;
      if (text[k] === ',') k++;
    } else {
      let d = 0;
      for (; k < close; k++) {
        if ('([{'.includes(text[k])) d++;
        else if (')]}'.includes(text[k])) d--;
        else if (d === 0 && text[k] === ',') {
          k++;
          break;
        }
      }
    }
  }
  return out;
}

/** The top-level `|` alternatives of a pattern, without a trailing `if` guard (which is reported separately). */
function alternatives(pattern) {
  const guard = /\s+if\s+/.exec(pattern);
  const text = guard ? pattern.slice(0, guard.index) : pattern;
  const parts = [];
  let depth = 0;
  let from = 0;
  for (let k = 0; k < text.length; k++) {
    if ('([{'.includes(text[k])) depth++;
    else if (')]}'.includes(text[k])) depth--;
    else if (depth === 0 && text[k] === '|') {
      parts.push(text.slice(from, k));
      from = k + 1;
    }
  }
  parts.push(text.slice(from));
  return { parts: parts.map((p) => p.trim()), guarded: Boolean(guard) };
}

/** Whether an unguarded arm pattern takes every value: `_`, or a plain binding (`other`, `ref x`, `mut y`). */
function catchAll(pattern) {
  const { parts, guarded } = alternatives(pattern);
  if (guarded) return false;
  return parts.some((p) => {
    const words = p.split(/\s+/).filter((w) => !KEYWORDS.has(w));
    return words.length === 1 && /^(?:_|[a-z_]\w*)$/.test(words[0]) && !['true', 'false'].includes(words[0]);
  });
}

/**
 * The matches on one of `names` (enum type names) in `source`: [{ line, enumName, variants, catchAllLine }], where `catchAllLine` is the
 * line of the catch-all arm or 0. Test code is left out: `#[cfg(test)]` items, or the whole file when `wholeFileIsTest`.
 */
export function enumMatches(source, names, { wholeFileIsTest = false } = {}) {
  if (wholeFileIsTest) return [];
  const masked = maskRust(source);
  const tests = rustTestRanges(masked);
  const lineOf = (k) => masked.slice(0, k).split('\n').length;
  const mention = new RegExp(`\\b(${names.map((n) => n.replace(/\W/g, '')).join('|')})::(\\w+)`, 'g');
  const out = [];
  for (const m of masked.matchAll(/\bmatch\b/g)) {
    if (tests.some(([a, b]) => m.index >= a && m.index < b)) continue;
    let depth = 0;
    let open = -1;
    for (let k = m.index + 5; k < masked.length; k++) {
      const c = masked[k];
      if (c === '(' || c === '[') depth++;
      else if (c === ')' || c === ']') depth--;
      else if (c === ';' && depth === 0) break;
      else if (c === '{' && depth === 0) {
        open = k;
        break;
      }
    }
    if (open < 0) continue;
    const close = closing(masked, open);
    if (close < 0) continue;
    const list = arms(masked, open, close);
    const variants = new Set();
    let enumName = null;
    for (const a of list) {
      for (const v of a.pattern.matchAll(mention)) {
        enumName ??= v[1];
        variants.add(v[2]);
      }
    }
    if (!variants.size) continue;
    const wild = list.find((a) => catchAll(a.pattern));
    out.push({ line: lineOf(m.index), enumName, variants: [...variants], catchAllLine: wild ? lineOf(wild.at) : 0 });
  }
  return out;
}
