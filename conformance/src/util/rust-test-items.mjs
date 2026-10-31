/**
 * `masked` (a `maskRust` result) with every test item blanked (`#[cfg(test)]` and `#[cfg(all(test, ..))]` items of any kind, `impl`
 * blocks included, and `#[test]` / `#[tokio::test]` style functions), newlines and offsets kept: a `mod name { … }` or fn
 * with a body (brace-matched) or a single item ending in `;` (a `use`, a `const`). Code after a test item stays visible, so a
 * leading `#[cfg(test)] use …;` hides nothing below it.
 */
export function withoutTestItems(masked) {
  const chars = [...masked];
  const blank = (from, to) => {
    for (let i = from; i < to; i++) if (chars[i] !== '\n') chars[i] = ' ';
  };
  for (const m of masked.matchAll(/#\[\s*(?:cfg\s*\(\s*(?:all\s*\(\s*)?test\b[^\]]*\)|(?:\w+::)*test\b[^\]]*)\]/g)) {
    let i = m.index + m[0].length;
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
    blank(m.index, end);
  }
  return chars.join('');
}
