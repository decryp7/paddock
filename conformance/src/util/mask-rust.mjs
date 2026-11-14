// Rust lexing for the conformance checks, a hand port of rustc's lexer (`ra-ap-rustc_lexer` 0.184, the crate the Rust host uses,
// tools/paddock-dev/src/check/lex/rust.rs). The Rust host is the oracle: tests/it/check_lex.rs compares the two on every tracked
// `.rs` file, on generated sources and on the review's probes. `maskRust` blanks comments and literal insides (C30, C39, C53, C120);
// `rustComments` keeps only the comments (the waiver reader). Unlike a regex scanner it knows nested block comments, `cr"..."`, raw
// strings of any prefix, lifetimes against char literals, raw identifiers and unterminated forms.

const WS = '\n\u000b\u000c\r\u0085  ‎‏\t ';
const isWs = (c) => c !== '' && WS.includes(c);
const isIdStart = (c) => c === '_' || /^\p{XID_Start}$/u.test(c);
const isIdContinue = (c) => /^\p{XID_Continue}$/u.test(c);
const isEmoji = (c) => c.codePointAt(0) > 127 && /^\p{Emoji}$/u.test(c);
const isDigit = (c) => c >= '0' && c <= '9' && c.length === 1;

class Cursor {
  constructor(chars) {
    this.s = chars;
    this.i = 0;
  }
  first() { return this.s[this.i] ?? '\0'; }
  second() { return this.s[this.i + 1] ?? '\0'; }
  third() { return this.s[this.i + 2] ?? '\0'; }
  eof() { return this.i >= this.s.length; }
  bump() { return this.i < this.s.length ? this.s[this.i++] : null; }
  eatWhile(pred) { while (pred(this.first()) && !this.eof()) this.i++; }
  eatUntil(c) { while (!this.eof() && this.s[this.i] !== c) this.i++; }
  eatIdentifier() {
    if (!isIdStart(this.first())) return;
    this.i++;
    this.eatWhile(isIdContinue);
  }
}

/** One token: { start, end, k, open?, close? } in code points. k: 'ws' | 'comment' | 'doc' | 'bracket' | 'lit' | 'other'. */
function advance(c, start) {
  const first = c.bump();
  const tok = (k, extra = {}) => ({ start, end: c.i, k, ...extra });
  const suffix = (terminated = true) => { const at = c.i; if (terminated) c.eatIdentifier(); return at; };
  switch (true) {
    case first === '/':
      if (c.first() === '/') {
        c.bump();
        const f = c.first();
        const doc = f === '!' || (f === '/' && c.second() !== '/');
        c.eatUntil('\n');
        return tok(doc ? 'doc' : 'comment');
      }
      if (c.first() === '*') {
        c.bump();
        const f = c.first();
        const doc = f === '!' || (f === '*' && c.second() !== '*' && c.second() !== '/');
        let depth = 1;
        for (let ch = c.bump(); ch !== null; ch = c.bump()) {
          if (ch === '/' && c.first() === '*') { c.bump(); depth++; }
          else if (ch === '*' && c.first() === '/') { c.bump(); if (--depth === 0) break; }
        }
        return tok(doc ? 'doc' : 'comment');
      }
      return tok('other');
    case isWs(first):
      c.eatWhile(isWs);
      return tok('ws');
    case first === 'r':
      if (c.first() === '#' && isIdStart(c.second())) { c.bump(); c.eatIdentifier(); return tok('other'); }
      if (c.first() === '#' || c.first() === '"') return rawLiteral(c, start, 1);
      return identOrPrefix(c, start);
    case first === 'k':
      if (c.first() === '#' && isIdStart(c.second())) { c.bump(); c.eatIdentifier(); return tok('other'); }
      return identOrPrefix(c, start);
    case first === 'b':
      return cOrByte(c, start, true);
    case first === 'c':
      return cOrByte(c, start, false);
    case isIdStart(first):
      return identOrPrefix(c, start);
    case isDigit(first):
      number(c, first);
      c.eatIdentifier();
      return tok('other');
    case first === '#' && (c.first() === '"' || c.first() === '#'):
      c.bump();
      return tok('other');
    case first === '[':
      return tok('bracket');
    case first === "'":
      return lifetimeOrChar(c, start);
    case first === '"': {
      const open = c.i - start;
      const terminated = doubleQuoted(c);
      const at = suffix(terminated);
      void at;
      return { start, end: c.i, k: 'lit', open, close: terminated ? at - start - 1 : c.i - start };
    }
    case isEmoji(first):
      c.eatWhile((ch) => isIdContinue(ch) || isEmoji(ch));
      return tok('other');
    default:
      return tok('other');
  }
}

function identOrPrefix(c, start) {
  c.eatWhile(isIdContinue);
  const f = c.first();
  if (f !== '#' && f !== '"' && f !== "'" && isEmoji(f)) c.eatWhile((ch) => isIdContinue(ch) || isEmoji(ch));
  return { start, end: c.i, k: 'other' };
}

function doubleQuoted(c) {
  for (let ch = c.bump(); ch !== null; ch = c.bump()) {
    if (ch === '"') return true;
    if (ch === '\\' && (c.first() === '\\' || c.first() === '"')) c.bump();
  }
  return false;
}

function singleQuoted(c) {
  if (c.second() === "'" && c.first() !== '\\') { c.bump(); c.bump(); return true; }
  for (;;) {
    const f = c.first();
    if (f === "'") { c.bump(); return true; }
    if (f === '/') break;
    if (f === '\n' && c.second() !== "'") break;
    if (f === '\0' && c.eof()) break;
    if (f === '\\') { c.bump(); c.bump(); } else c.bump();
  }
  return false;
}

/** After the `r` (already eaten): the hashes, the quote and the body up to the closer. Returns the token with `open` and `close`. */
function rawLiteral(c, start, prefixLen) {
  const hashesAt = c.i;
  let hashes = 0;
  while (c.first() === '#') { hashes++; c.bump(); }
  const open = c.i - start + 1;
  void hashesAt;
  const q = c.bump();
  if (q !== '"') {
    // An invalid starter (`r#x`): the bad character is eaten too, and it is no literal.
    return { start, end: c.i, k: 'other' };
  }
  for (;;) {
    c.eatUntil('"');
    if (c.eof()) return { start, end: c.i, k: 'lit', open, close: c.i - start }; // no terminator: the body runs to the end
    c.bump();
    let n = 0;
    while (c.first() === '#' && n < hashes) { n++; c.bump(); }
    if (n === hashes) {
      if (hashes > 255) return { start, end: c.i, k: 'lit', open, close: c.i - start }; // too many delimiters: no suffix, the closer is inside
      const closeEnd = c.i;
      c.eatIdentifier();
      return { start, end: c.i, k: 'lit', open, close: closeEnd - start - 1 - hashes };
    }
  }
  void prefixLen;
}

function cOrByte(c, start, byte) {
  const f = c.first();
  if (f === "'" && byte) {
    c.bump();
    const open = c.i - start;
    const terminated = singleQuoted(c);
    const at = c.i;
    if (terminated) c.eatIdentifier();
    return terminated ? { start, end: c.i, k: 'lit', open, close: at - start - 1 } : { start, end: c.i, k: 'other' };
  }
  if (f === '"') {
    c.bump();
    const open = c.i - start;
    const terminated = doubleQuoted(c);
    const at = c.i;
    if (terminated) c.eatIdentifier();
    return { start, end: c.i, k: 'lit', open, close: terminated ? at - start - 1 : c.i - start };
  }
  if (f === 'r' && (c.second() === '"' || c.second() === '#')) {
    c.bump();
    return rawLiteral(c, start, 2);
  }
  return identOrPrefix(c, start);
}

function lifetimeOrChar(c, start) {
  let invalid = false;
  let canBeLifetime;
  if (c.second() === "'") canBeLifetime = false;
  else {
    const f = c.first();
    invalid = isDigit(f) || isEmoji(f);
    canBeLifetime = isIdStart(f) || invalid;
  }
  if (!canBeLifetime) {
    const open = c.i - start;
    const terminated = singleQuoted(c);
    const at = c.i;
    if (terminated) c.eatIdentifier();
    return terminated ? { start, end: c.i, k: 'lit', open, close: at - start - 1 } : { start, end: c.i, k: 'other' };
  }
  if (c.first() === 'r' && c.second() === '#' && isIdStart(c.third())) {
    c.bump(); c.bump(); c.bump();
    c.eatWhile(isIdContinue);
    return { start, end: c.i, k: 'other' };
  }
  const startsWithNumber = isDigit(c.first());
  c.bump();
  c.eatWhile(isIdContinue);
  { const f = c.first(); if (f !== '#' && f !== '"' && f !== "'" && isEmoji(f)) c.eatWhile((ch) => isIdContinue(ch) || isEmoji(ch)); }
  if (c.first() === "'") {
    c.bump();
    return { start, end: c.i, k: 'lit', open: 1, close: c.i - start - 1 }; // a multi-character char literal: no suffix is read
  }
  void startsWithNumber;
  return { start, end: c.i, k: 'other' };
}

function number(c, firstDigit) {
  const eatDecimal = () => { let any = false; for (;;) { const f = c.first(); if (f === '_') c.bump(); else if (isDigit(f)) { c.bump(); any = true; } else break; } return any; };
  const eatHex = () => { let any = false; for (;;) { const f = c.first(); if (f === '_') c.bump(); else if (/^[0-9a-fA-F]$/.test(f)) { c.bump(); any = true; } else break; } return any; };
  const eatExponent = () => {
    if (c.first() === '-' || c.first() === '+') c.bump();
    return eatDecimal();
  };
  if (firstDigit === '0') {
    const f = c.first();
    if (f === 'b' || f === 'o') { c.bump(); if (!eatDecimal()) return; }
    else if (f === 'x') { c.bump(); if (!eatHex()) return; }
    else if (isDigit(f) || f === '_') eatDecimal();
    else if (f === '.' || f === 'e' || f === 'E') { /* not a base prefix */ }
    else return;
  } else eatDecimal();
  const f = c.first();
  if (f === '.' && c.second() !== '.' && !isIdStart(c.second())) {
    c.bump();
    if (isDigit(c.first())) {
      eatDecimal();
      if (c.first() === 'e' || c.first() === 'E') { c.bump(); eatExponent(); }
    }
  } else if (f === 'e' || f === 'E') { c.bump(); eatExponent(); }
}

/** The tokens of a Rust source (code points; a shebang line is one 'other' token; no frontmatter). */
export function lexRust(text) {
  const chars = Array.from(text);
  const out = [];
  let pos = 0;
  if (text.startsWith('#!')) {
    // A shebang unless the next significant token after `#!` is `[`.
    const tail = new Cursor(chars.slice(2));
    let next = null;
    for (; !tail.eof(); ) {
      const t = advance(tail, tail.i);
      if (t.k !== 'ws' && t.k !== 'comment') { next = t; break; }
    }
    if (!next || next.k !== 'bracket') {
      const rest = chars.slice(2).join('');
      const line = rest.split(/\n/)[0].replace(/\r$/, '');
      pos = 2 + Array.from(line).length;
      out.push({ start: 0, end: pos, k: 'other' });
    }
  }
  const c = new Cursor(chars);
  c.i = pos;
  while (!c.eof()) {
    const t = advance(c, c.i);
    out.push(t);
  }
  return { chars, tokens: out };
}

const blankCp = (ch) => (ch === '\n' ? '\n' : ' '.repeat(ch.length));

/** `text` with comments and the insides of literals blanked (newlines kept, one blank per UTF-16 unit). */
export function maskRust(text) {
  const { chars, tokens } = lexRust(text);
  const blank = new Array(chars.length).fill(false);
  for (const t of tokens) {
    if (t.k === 'comment' || t.k === 'doc') for (let i = t.start; i < t.end; i++) blank[i] = true;
    else if (t.k === 'lit') for (let i = t.start + t.open; i < t.start + t.close; i++) blank[i] = true;
  }
  return chars.map((ch, i) => (blank[i] ? blankCp(ch) : ch)).join('');
}

/** Only the comments of a Rust source kept; everything else blanked (newlines kept, same length in UTF-16 units). */
export function rustComments(text) {
  const { chars, tokens } = lexRust(text);
  const keep = new Array(chars.length).fill(false);
  for (const t of tokens) if (t.k === 'comment' || t.k === 'doc') for (let i = t.start; i < t.end; i++) keep[i] = true;
  return chars.map((ch, i) => (keep[i] || ch === '\n' ? ch : blankCp(ch))).join('');
}
