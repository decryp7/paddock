// C39: tests never free a port they probe (rule 40, ADR 0023). A port read from a listener that is then dropped is free
// again: the OS may hand it to another process (on a shared CI runner, a concurrent test's server) before the test connects,
// and the test then talks to that server. Flagged, in any Rust file:
// - a chain that reads the port off a temporary listener: `TcpListener::bind(..).unwrap().local_addr()` (also `?`, `.expect(..)`,
//   `.await`);
// - a block (a `{ .. }` expression or a fn body) that binds `let l = ..bind(..)` and ends with `l.local_addr()..`, using `l`
//   for nothing else: the listener drops when the block ends;
// - `drop(l)` of a listener whose `local_addr()` was read before.
// The fix: `paddock_testkit::net::HeldPort` keeps the port taken (it hangs up or resets), and a test that needs "refused"
// injects a connector or tests the error mapping. A justified case carries `// conformance: freed-port — <reason>` on the
// line or the line above.
import { error } from '../finding.mjs';

const WAIVER = /conformance:\s*freed-port\s*[—–-]+\s*(\S.{2,})/;
const BARE_WAIVER = /conformance:\s*freed-port/;
const BIND = /\b(?:TcpListener|TcpSocket)::bind\s*\(/g;

/**
 * `text` with comments and the insides of string, raw-string and char literals blanked (newlines kept). Unlike C30's mask it
 * knows raw strings (`r#"..."#`) and char literals (`'"'`), which would otherwise open a string that never ends.
 */
export function maskRust(text) {
  let out = '';
  const blank = (s) => s.replace(/[^\n]/g, ' ');
  for (let i = 0; i < text.length; ) {
    const rest = text.slice(i, i + 12);
    const raw = /^b?r(#*)"/.exec(rest);
    const prev = text[i - 1] ?? ' ';
    if (raw && !/\w/.test(prev)) {
      const close = `"${raw[1]}`;
      const end = text.indexOf(close, i + raw[0].length);
      const stop = end < 0 ? text.length : end + close.length;
      out += raw[0] + blank(text.slice(i + raw[0].length, stop - close.length)) + close;
      i = stop;
    } else if (text.startsWith('//', i)) {
      const end = text.indexOf('\n', i);
      const stop = end < 0 ? text.length : end;
      out += ' '.repeat(stop - i);
      i = stop;
    } else if (text.startsWith('/*', i)) {
      const end = text.indexOf('*/', i + 2);
      const stop = end < 0 ? text.length : end + 2;
      out += blank(text.slice(i, stop));
      i = stop;
    } else if (text[i] === '"') {
      let j = i + 1;
      while (j < text.length && text[j] !== '"') j += text[j] === '\\' ? 2 : 1;
      out += `"${blank(text.slice(i + 1, j))}"`;
      i = j + 1;
    } else if (text[i] === "'") {
      const ch = /^'(?:\\u\{[0-9a-fA-F]+\}|\\.|[^\\'\n])'/.exec(text.slice(i, i + 12));
      if (ch) {
        out += `'${' '.repeat(ch[0].length - 2)}'`;
        i += ch[0].length;
      } else {
        out += "'";
        i++;
      }
    } else {
      out += text[i];
      i++;
    }
  }
  return out;
}

/** Index of the bracket closing the one at `open` in masked text, or -1. */
function closing(text, open) {
  const o = text[open];
  const c = { '(': ')', '{': '}', '[': ']' }[o];
  let depth = 0;
  for (let i = open; i < text.length; i++) {
    if (text[i] === o) depth++;
    else if (text[i] === c && --depth === 0) return i;
  }
  return -1;
}

/** Index of the `{` of the innermost block that encloses `at`, or -1. */
function enclosingBlock(text, at) {
  let depth = 0;
  for (let i = at - 1; i >= 0; i--) {
    if (text[i] === '}') depth++;
    else if (text[i] === '{') {
      if (depth === 0) return i;
      depth--;
    }
  }
  return -1;
}

/** The end of a call's "unwrap tail" after `i`: `.unwrap()`, `.expect(..)`, `?`, `.await`, in any order. */
function skipTail(text, i) {
  for (;;) {
    const rest = text.slice(i);
    const m = /^\s*(?:\?|\.\s*await\b|\.\s*unwrap\s*\(\s*\)|\.\s*expect\s*\()/.exec(rest);
    if (!m) return i;
    i += m[0].length;
    if (m[0].endsWith('(') && !m[0].endsWith('()')) {
      const end = closing(text, i - 1);
      if (end < 0) return i;
      i = end + 1;
    }
  }
}

const esc = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/** The freed ports in a Rust source: [{ line, kind, waived: false | 'no-reason' | 'reason' }]. */
export function freedPorts(text) {
  const clean = maskRust(text);
  const raw = text.split('\n');
  const out = [];
  const lineOf = (i) => clean.slice(0, i).split('\n').length;
  const push = (i, kind) => {
    const line = lineOf(i);
    const here = `${raw[line - 1] ?? ''}\n${raw[line - 2] ?? ''}`;
    const waived = WAIVER.test(here) ? 'reason' : BARE_WAIVER.test(here) ? 'no-reason' : false;
    out.push({ line, kind, waived });
  };
  for (const m of clean.matchAll(BIND)) {
    const open = m.index + m[0].length - 1;
    const close = closing(clean, open);
    if (close < 0) continue;
    const after = skipTail(clean, close + 1);
    if (/^\s*\.\s*local_addr\s*\(/.test(clean.slice(after))) {
      push(m.index, 'chain');
      continue;
    }
    // `let NAME = ...bind(..)` as a statement.
    const stmtStart = Math.max(clean.lastIndexOf(';', m.index), clean.lastIndexOf('{', m.index), clean.lastIndexOf('}', m.index));
    const letM = /^\s*let\s+(?:mut\s+)?(\w+)\s*(?::[^=]+)?=/.exec(clean.slice(stmtStart + 1, m.index));
    if (!letM) continue;
    const name = letM[1];
    const semi = clean.indexOf(';', after);
    if (semi < 0) continue;
    const block = enclosingBlock(clean, m.index);
    if (block < 0) continue;
    const end = closing(clean, block);
    if (end < 0) continue;
    const body = clean.slice(semi + 1, end);
    const uses = [...body.matchAll(new RegExp(`\\b${esc(name)}\\b`, 'g'))];
    const portReads = [...body.matchAll(new RegExp(`\\b${esc(name)}\\s*\\.\\s*local_addr\\s*\\(`, 'g'))];
    const dropAt = new RegExp(`\\bdrop\\s*\\(\\s*${esc(name)}\\s*\\)`).exec(body);
    if (dropAt) {
      // The port (or a value made from it) is used after the drop.
      const before = body.slice(0, dropAt.index);
      const later = body.slice(dropAt.index);
      const derived = [...before.matchAll(new RegExp(`\\blet\\s+(?:mut\\s+)?(\\w+)[^;=]*=[^;]*\\b${esc(name)}\\s*\\.\\s*local_addr\\s*\\(`, 'g'))].map((d) => d[1]);
      if (derived.some((d) => new RegExp(`\\b${esc(d)}\\b`).test(later.split(new RegExp(`\\blet\\s+(?:mut\\s+)?${esc(d)}\\b`))[0]))) push(semi + 1 + dropAt.index, 'drop');
      continue;
    }
    const tail = body.slice(body.lastIndexOf(';') + 1);
    const tailRead = new RegExp(`\\b${esc(name)}\\s*\\.\\s*local_addr\\s*\\(`).test(tail);
    if (tailRead && uses.length === portReads.length) push(m.index, 'block');
  }
  return out;
}

export default {
  id: 'C39',
  title: 'Tests never free a port they probe',
  rule: 40,
  run(ctx) {
    const out = [];
    const fix =
      'Keep the port taken: `paddock_testkit::net::HeldPort::hang_up()` / `::reset()`; for "refused", inject a connector or test the error mapping (ADR 0023). A justified case carries `// conformance: freed-port — <reason>`.';
    for (const f of ctx.files()) {
      if (!f.endsWith('.rs')) continue;
      const text = ctx.read(f);
      if (!text || !/::bind\s*\(/.test(text)) continue;
      for (const p of freedPorts(text)) {
        const what = { chain: 'reads the port of a listener it drops at once', block: 'returns the port of a listener the block drops', drop: 'drops a listener whose port it read' }[p.kind];
        if (p.waived === 'reason') continue;
        if (p.waived === 'no-reason') out.push(error('C39', 40, f, p.line, `the freed-port waiver gives no reason`, 'Write `// conformance: freed-port — <why the port may be freed>`.'));
        else out.push(error('C39', 40, f, p.line, `${what}: another process may be given the port before the test uses it`, fix));
      }
    }
    return out;
  },
};
