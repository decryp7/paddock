// YAML comments, a hand reader held to tree-sitter-yaml (the Rust host's parser, tools/paddock-dev/src/check/lex/yaml.rs) by the parity
// tests. Block style: `key: value`, `- item`, plain scalars (an apostrophe inside one opens no quote), single and double quoted scalars (also
// over several lines), block scalars (`|` and `>`: their content is a string, a `#` there is no comment) and `#` after white space.
// Flow collections, anchors with values on the next line and multi-document streams are not claimed to match.

/** The ranges [start, end) (UTF-16 units) of the comments of a YAML document. */
export function yamlCommentRanges(text) {
  const ranges = [];
  const lines = text.split('\n');
  let offset = 0;
  let block = null; // { parent } while inside a block scalar
  let quote = null; // "'" or '"' while inside a quoted scalar that continues on this line
  for (const raw of lines) {
    const line = raw;
    const here = offset;
    offset += raw.length + 1;
    const indent = line.length - line.trimStart().length;
    if (block) {
      if (line.trim() === '' || indent > block.parent) continue;
      block = null;
    }
    let i = 0;
    let atStart = true;
    let keyCol = indent;
    if (quote) {
      i = closeQuote(line, 0, quote);
      if (i < 0) continue;
      quote = null;
      atStart = false;
    } else {
      i = indent;
    }
    while (i < line.length) {
      const c = line[i];
      if (c === '#' && (i === 0 || line[i - 1] === ' ' || line[i - 1] === '\t')) {
        ranges.push([here + i, here + line.length]);
        break;
      }
      if (atStart) {
        if (c === '-' && (line[i + 1] === ' ' || line[i + 1] === '\t' || i + 1 >= line.length)) {
          i++;
          while (line[i] === ' ' || line[i] === '\t') i++;
          keyCol = i;
          continue;
        }
        if (c === '"' || c === "'") {
          const e = closeQuote(line, i + 1, c);
          if (e < 0) { quote = c; i = line.length; } else i = e;
          atStart = false;
          continue;
        }
        if (c === '|' || c === '>') {
          const m = /^[|>][+\-0-9]*[ \t]*(#.*)?$/.exec(line.slice(i));
          if (m) {
            if (m[1]) ranges.push([here + line.length - m[1].length, here + line.length]);
            block = { parent: keyCol };
            i = line.length;
            break;
          }
        }
        atStart = false;
      }
      if (c === ':' && (i + 1 >= line.length || line[i + 1] === ' ' || line[i + 1] === '\t')) {
        atStart = true;
        i++;
        while (line[i] === ' ' || line[i] === '\t') i++;
        continue;
      }
      i++;
    }
  }
  return ranges;
}

/** The index after the quote that closes the scalar started with `q`, searching from `from`; -1 when it does not close on this line. */
function closeQuote(line, from, q) {
  for (let i = from; i < line.length; i++) {
    if (q === '"' && line[i] === '\\') i++;
    else if (line[i] === q) {
      if (q === "'" && line[i + 1] === "'") i++;
      else return i + 1;
    }
  }
  return -1;
}
