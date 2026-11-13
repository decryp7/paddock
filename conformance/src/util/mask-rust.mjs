// `maskRust`: Rust source with comments and the insides of string, raw-string and char literals blanked. Shared by C39, C53, C120 and
// the inline-waiver reader (a waiver marker counts only in a comment). Rust host: tools/paddock-dev/src/check/mask.rs.
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

/**
 * The complement of `maskRust`: only the comments of a Rust source are kept; code, strings, raw strings and char literals are
 * blanked (newlines kept, same length). The waiver reader looks at this text only, so a marker inside a literal is data, not a waiver.
 * Rust host: `comment_units` in tools/paddock-dev/src/check/mask.rs.
 */
export function rustComments(text) {
  let out = '';
  const blank = (s) => s.replace(/[^\n]/g, ' ');
  for (let i = 0; i < text.length; ) {
    const raw = /^b?r(#*)"/.exec(text.slice(i, i + 12));
    const prev = text[i - 1] ?? ' ';
    if (raw && !/\w/.test(prev)) {
      const close = `"${raw[1]}`;
      const end = text.indexOf(close, i + raw[0].length);
      const stop = end < 0 ? text.length : end + close.length;
      out += blank(text.slice(i, stop));
      i = stop;
    } else if (text.startsWith('//', i)) {
      const end = text.indexOf('\n', i);
      const stop = end < 0 ? text.length : end;
      out += text.slice(i, stop);
      i = stop;
    } else if (text.startsWith('/*', i)) {
      const end = text.indexOf('*/', i + 2);
      const stop = end < 0 ? text.length : end + 2;
      out += text.slice(i, stop);
      i = stop;
    } else if (text[i] === '"') {
      let j = i + 1;
      while (j < text.length && text[j] !== '"') j += text[j] === '\\' ? 2 : 1;
      out += blank(text.slice(i, j + 1));
      i = j + 1;
    } else if (text[i] === "'") {
      const ch = /^'(?:\\u\{[0-9a-fA-F]+\}|\\.|[^\\'\n])'/.exec(text.slice(i, i + 12));
      out += ch ? blank(ch[0]) : ' ';
      i += ch ? ch[0].length : 1;
    } else {
      out += text[i] === '\n' ? '\n' : ' ';
      i++;
    }
  }
  return out;
}
