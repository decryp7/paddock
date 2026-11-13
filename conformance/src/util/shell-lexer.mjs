// Shell comments, a hand reader held to tree-sitter-bash (the Rust host's parser, tools/paddock-dev/src/check/lex/shell.rs) by the
// parity tests. It knows single and double quotes, `$'...'` with its escapes, `${...}`, `$#`-style parameters, escapes, here-documents
// (`<<EOF`, `<<-EOF`, quoted delimiters; the rest of the line is read normally, the body from the next line) and a `#` that starts a word.
// A construct beyond that (arithmetic `<<`, process substitution, here-strings inside nested substitutions) is not claimed to match.

const WORD_START = new Set([' ', '\t', '\n', ';', '&', '|', '(', ')', '\r']);

/** The ranges [start, end) (UTF-16 units) of the comments of a shell script. */
export function shellCommentRanges(text) {
  const ranges = [];
  const n = text.length;
  let pending = []; // here-document delimiters waiting for the end of the line: { word, strip }
  let i = 0;
  const skipHeredocBodies = () => {
    for (const { word, strip } of pending) {
      for (;;) {
        if (i >= n) break;
        let e = text.indexOf('\n', i);
        if (e < 0) e = n;
        let line = text.slice(i, e);
        if (line.endsWith('\r')) line = line.slice(0, -1);
        if (strip) line = line.replace(/^\t+/, '');
        i = Math.min(e + 1, n);
        if (line === word) break;
      }
    }
    pending = [];
  };
  while (i < n) {
    const c = text[i];
    if (c === '\n') {
      i++;
      if (pending.length) skipHeredocBodies();
    } else if (c === '\\') i += 2;
    else if (c === "'") {
      const e = text.indexOf("'", i + 1);
      i = e < 0 ? n : e + 1;
    } else if (c === '"') {
      i++;
      while (i < n && text[i] !== '"') i += text[i] === '\\' ? 2 : 1;
      i++;
    } else if (c === '$' && text[i + 1] === "'") {
      i += 2;
      while (i < n && text[i] !== "'") i += text[i] === '\\' ? 2 : 1;
      i++;
    } else if (c === '$' && text[i + 1] === '{') {
      let depth = 0;
      for (; i < n; i++) {
        if (text[i] === '{') depth++;
        else if (text[i] === '}' && --depth === 0) { i++; break; }
      }
    } else if (c === '$') i += 2;
    else if (c === '#' && (i === 0 || WORD_START.has(text[i - 1]))) {
      let e = text.indexOf('\n', i);
      if (e < 0) e = n;
      ranges.push([i, e]);
      i = e;
    } else if (c === '<' && text[i + 1] === '<' && text[i + 2] !== '<') {
      i += 2;
      let strip = false;
      if (text[i] === '-') { strip = true; i++; }
      while (text[i] === ' ' || text[i] === '\t') i++;
      let word = '';
      const q = text[i];
      if (q === "'" || q === '"') {
        const e = text.indexOf(q, i + 1);
        word = text.slice(i + 1, e < 0 ? n : e);
        i = e < 0 ? n : e + 1;
      } else {
        const m = /^[^\s;&|()<>]+/.exec(text.slice(i));
        word = m ? m[0] : '';
        i += word.length;
      }
      if (word) pending.push({ word, strip });
    } else i++;
  }
  return ranges;
}
