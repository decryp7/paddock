// A small TOML reader for the register files of ADR 0052 (features.toml, wiring.toml): `[[array]]` and `[table]` headers,
// `key = value` with a basic string, an integer, a boolean, an array (also over several lines) and an inline table, and `#`
// comments. It is not a full TOML parser (no dotted keys, no multi-line strings, no dates); anything it cannot read is
// reported with its line, never skipped, so a register cannot lose a row silently (ER-33). The conformance package has no
// dependencies, so this is the one reader for these files.

/** Index of the end of a value that starts at `i` in `s` (exclusive), or -1 when it is not complete. */
function valueEnd(s, i) {
  let depth = 0;
  let inStr = false;
  for (let j = i; j < s.length; j++) {
    const c = s[j];
    if (inStr) {
      if (c === '\\') j++;
      else if (c === '"') {
        inStr = false;
        if (depth === 0) return j + 1;
      }
    } else if (c === '"') inStr = true;
    else if (c === '[' || c === '{') depth++;
    else if (c === ']' || c === '}') {
      depth--;
      if (depth === 0) return j + 1;
    } else if (depth === 0 && (c === ',' || c === '#' || c === '\n')) return j;
  }
  return depth === 0 && !inStr ? s.length : -1;
}

/** Parses the value at the start of `s` (after trimming); returns { value, rest }. Throws Error with a reason. */
function parseValue(s) {
  s = s.replace(/^\s+/, '');
  const c = s[0];
  if (c === '"') {
    const end = valueEnd(s, 0);
    if (end < 0) throw new Error('a string is not closed');
    let value;
    try {
      value = JSON.parse(s.slice(0, end));
    } catch {
      throw new Error(`a string cannot be read: ${s.slice(0, end)}`);
    }
    return { value, rest: s.slice(end) };
  }
  if (c === '[') {
    const out = [];
    let rest = s.slice(1);
    for (;;) {
      rest = rest.replace(/^(\s|#[^\n]*)+/, '');
      if (rest[0] === ']') return { value: out, rest: rest.slice(1) };
      const r = parseValue(rest);
      out.push(r.value);
      rest = r.rest.replace(/^(\s|#[^\n]*)+/, '');
      if (rest[0] === ',') rest = rest.slice(1);
      else if (rest[0] !== ']') throw new Error('an array needs "," or "]" after a value');
    }
  }
  if (c === '{') {
    const out = {};
    let rest = s.slice(1);
    for (;;) {
      rest = rest.replace(/^\s+/, '');
      if (rest[0] === '}') return { value: out, rest: rest.slice(1) };
      const k = /^([A-Za-z0-9_-]+)\s*=\s*/.exec(rest);
      if (!k) throw new Error('an inline table needs `key = value`');
      const r = parseValue(rest.slice(k[0].length));
      out[k[1]] = r.value;
      rest = r.rest.replace(/^\s+/, '');
      if (rest[0] === ',') rest = rest.slice(1);
      else if (rest[0] !== '}') throw new Error('an inline table needs "," or "}" after a value');
    }
  }
  const m = /^(true|false|-?\d+)(?![\w.])/.exec(s);
  if (m) return { value: m[1] === 'true' ? true : m[1] === 'false' ? false : Number(m[1]), rest: s.slice(m[0].length) };
  throw new Error(`a value cannot be read: ${s.split('\n')[0].slice(0, 40)}`);
}

/**
 * Reads a register file. Returns { tables, problems }: `tables` maps a name to an array of rows (`[[name]]`) or one row
 * (`[name]`), each row `{ ...keys, __line }`; top-level keys go to `tables['']`. `problems` is [{ line, message }].
 */
export function parseToml(text) {
  const tables = { '': { __line: 1 } };
  const problems = [];
  const lines = text.split('\n');
  let cur = tables[''];
  for (let n = 0; n < lines.length; n++) {
    const raw = lines[n];
    const t = raw.replace(/^\s+/, '');
    if (t === '' || t[0] === '#') continue;
    const arr = /^\[\[\s*([A-Za-z0-9_.-]+)\s*\]\]\s*(#.*)?$/.exec(t);
    const tab = /^\[\s*([A-Za-z0-9_.-]+)\s*\]\s*(#.*)?$/.exec(t);
    if (arr) {
      cur = { __line: n + 1 };
      (tables[arr[1]] ??= []).push(cur);
      continue;
    }
    if (tab) {
      cur = { __line: n + 1 };
      tables[tab[1]] = cur;
      continue;
    }
    const kv = /^([A-Za-z0-9_-]+)\s*=\s*/.exec(t);
    if (!kv) {
      problems.push({ line: n + 1, message: `cannot read this line: ${t.slice(0, 60)}` });
      continue;
    }
    // A value can run over several lines (an array): take lines until it is complete.
    let body = t.slice(kv[0].length);
    let end = valueEnd(body, 0);
    const start = n;
    while (end < 0 && n + 1 < lines.length) body += `\n${lines[++n]}`, (end = valueEnd(body, 0));
    try {
      const r = parseValue(body);
      if (!/^\s*(#.*)?$/.test(r.rest)) throw new Error(`unexpected text after the value: ${r.rest.trim().slice(0, 30)}`);
      if (kv[1] in cur) throw new Error(`the key ${kv[1]} is set twice`);
      cur[kv[1]] = r.value;
    } catch (e) {
      problems.push({ line: start + 1, message: `${kv[1]}: ${e.message}` });
    }
  }
  return { tables, problems };
}
