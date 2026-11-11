// A small TOML reader for the register files of ADR 0052 (features.toml, wiring.toml): `[[array]]` and `[table]` headers,
// `key = value` with a basic string, an integer, a boolean, an array (also over several lines) and an inline table, and `#`
// comments. It is not a full TOML parser (no dotted keys, no multi-line strings, no dates); anything it cannot read is
// reported with its line, never skipped, so a register cannot lose a row silently (ER-33). The conformance package has no
// dependencies, so this is the one reader for these files.
//
// Hardening (it reads files from a branch under review): tables and rows have no prototype, so a key such as `__proto__`
// or `constructor` is data; input over 1 MiB is refused; nesting is capped; reading is linear in the size of the input
// (an index walks the text, nothing is re-sliced per token). A continuation of an unclosed array stops at the next line
// that starts in column 0 with a header or a `key =`, so one bad value never swallows the rest of the file.
// It lives beside the check that uses it (not in conformance/src/util/, a change there marks every consumer in the planner).

export const MAX_BYTES = 1 << 20;
const MAX_DEPTH = 16;

const KEY = /([A-Za-z0-9_-]+)\s*=\s*/y;
const SCALAR = /(true|false|-?\d+)(?![\w.])/y;

/** Index after the whitespace (and, when `comments`, the `#` comments) at `i`. */
function skip(s, i, comments, ops) {
  for (;;) {
    ops.n++;
    const c = s[i];
    if (c === ' ' || c === '\t' || c === '\r' || c === '\n') i++;
    else if (comments && c === '#') {
      while (i < s.length && s[i] !== '\n') (i++, ops.n++);
    } else return i;
  }
}

/** Parses the value at `i`; returns { value, i } past it. Throws Error with a reason. */
function parseValue(s, i, ops, depth) {
  i = skip(s, i, false, ops);
  const c = s[i];
  if (c === '"') {
    let j = i + 1;
    for (; j < s.length && s[j] !== '"'; j++) {
      ops.n++;
      if (s[j] === '\\') j++;
      else if (s[j] === '\n') break;
    }
    if (s[j] !== '"') throw new Error('a string is not closed');
    const raw = s.slice(i, j + 1);
    try {
      return { value: JSON.parse(raw), i: j + 1 };
    } catch {
      throw new Error(`a string cannot be read: ${raw.slice(0, 40)}`);
    }
  }
  if (c === '[' || c === '{') {
    if (depth >= MAX_DEPTH) throw new Error(`nested more than ${MAX_DEPTH} levels`);
    const isArr = c === '[';
    const close = isArr ? ']' : '}';
    const out = isArr ? [] : Object.create(null);
    i++;
    for (;;) {
      i = skip(s, i, isArr, ops);
      if (i >= s.length) throw new Error(isArr ? 'an array is not closed' : 'an inline table is not closed');
      if (s[i] === close) return { value: out, i: i + 1 };
      let key;
      if (!isArr) {
        KEY.lastIndex = i;
        const k = KEY.exec(s);
        if (!k) throw new Error('an inline table needs `key = value`');
        key = k[1];
        i += k[0].length;
      }
      const r = parseValue(s, i, ops, depth + 1);
      if (isArr) out.push(r.value);
      else {
        if (Object.hasOwn(out, key)) throw new Error(`the key ${key} is set twice in the inline table`);
        out[key] = r.value;
      }
      i = skip(s, r.i, isArr, ops);
      if (s[i] === ',') i++;
      else if (s[i] !== close) throw new Error(isArr ? 'an array needs "," or "]" after a value' : 'an inline table needs "," or "}" after a value');
    }
  }
  SCALAR.lastIndex = i;
  const m = SCALAR.exec(s);
  if (m) return { value: m[1] === 'true' ? true : m[1] === 'false' ? false : Number(m[1]), i: i + m[0].length };
  throw new Error(`a value cannot be read: ${s.slice(i).split('\n')[0].slice(0, 40)}`);
}

/**
 * Scans the rest of one line of a value: updates `st` ({ depth, inStr }) and returns an error text for a string that
 * runs past its line, else null. Stops at a `#` outside a string.
 */
function scanLine(line, st, ops) {
  for (let j = 0; j < line.length; j++) {
    ops.n++;
    const c = line[j];
    if (st.inStr) {
      if (c === '\\') j++;
      else if (c === '"') st.inStr = false;
    } else if (c === '"') st.inStr = true;
    else if (c === '#') break;
    else if (c === '[' || c === '{') st.depth++;
    else if (c === ']' || c === '}') st.depth--;
  }
  return st.inStr ? 'a string is not closed' : null;
}

const HEADER_AT_COLUMN_0 = /^(\[\[?\s*[A-Za-z0-9_.-]+\s*\]\]?|[A-Za-z0-9_-]+\s*=)/;

/**
 * Reads a register file. Returns { tables, problems }: `tables` maps a name to an array of rows (`[[name]]`) or one row
 * (`[name]`), each row `{ ...keys, __line }`; top-level keys go to `tables['']`. `problems` is [{ line, message }].
 * `opts.ops` (an object with a number `n`) counts the steps taken, so a test can bound the work without a clock.
 */
export function parseToml(text, opts = {}) {
  const ops = opts.ops ?? { n: 0 };
  const tables = Object.create(null);
  const problems = [];
  tables[''] = Object.assign(Object.create(null), { __line: 1 });
  if (text.length > MAX_BYTES) {
    problems.push({ line: 1, message: `the file is larger than ${MAX_BYTES} bytes (1 MiB); a register is never that big` });
    return { tables, problems };
  }
  const lines = text.split('\n');
  let cur = tables[''];
  for (let n = 0; n < lines.length; n++) {
    ops.n++;
    const t = lines[n].replace(/^\s+/, '');
    if (t === '' || t[0] === '#') continue;
    const arr = /^\[\[\s*([A-Za-z0-9_.-]+)\s*\]\]\s*(#.*)?$/.exec(t);
    const tab = arr ? null : /^\[\s*([A-Za-z0-9_.-]+)\s*\]\s*(#.*)?$/.exec(t);
    if (arr) {
      cur = Object.assign(Object.create(null), { __line: n + 1 });
      if (Object.hasOwn(tables, arr[1]) && !Array.isArray(tables[arr[1]])) problems.push({ line: n + 1, message: `[[${arr[1]}]] was already a [${arr[1]}] table` });
      else (tables[arr[1]] ??= []).push(cur);
      continue;
    }
    if (tab) {
      cur = Object.assign(Object.create(null), { __line: n + 1 });
      if (Object.hasOwn(tables, tab[1])) problems.push({ line: n + 1, message: `the table [${tab[1]}] appears twice (the second one would hide the first)` });
      else tables[tab[1]] = cur;
      continue;
    }
    const kv = /^([A-Za-z0-9_-]+)\s*=\s*/.exec(t);
    if (!kv) {
      problems.push({ line: n + 1, message: `cannot read this line: ${t.slice(0, 60)}` });
      continue;
    }
    // A value can run over several lines (an array): take lines until it is complete, never past a header or a `key =` in column 0.
    const start = n;
    const parts = [t.slice(kv[0].length)];
    const st = { depth: 0, inStr: false };
    let bad = scanLine(parts[0], st, ops);
    while (!bad && st.depth > 0) {
      if (n + 1 >= lines.length || HEADER_AT_COLUMN_0.test(lines[n + 1])) {
        bad = st.depth > 0 ? 'a value is not closed before the next header or key' : null;
        break;
      }
      parts.push(lines[++n]);
      bad = scanLine(lines[n], st, ops);
    }
    try {
      if (bad) throw new Error(bad);
      const body = parts.join('\n');
      const r = parseValue(body, 0, ops, 0);
      const rest = body.slice(r.i);
      if (!/^[ \t\r]*(#.*)?$/.test(rest)) throw new Error(`unexpected text after the value: ${rest.trim().slice(0, 30)}`);
      if (Object.hasOwn(cur, kv[1])) throw new Error(`the key ${kv[1]} is set twice`);
      cur[kv[1]] = r.value;
    } catch (e) {
      problems.push({ line: start + 1, message: `${kv[1]}: ${e.message}` });
    }
  }
  return { tables, problems };
}
