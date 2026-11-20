// C143 (ER-21 class "a program path that is not a path": `join('.', 'dev')` returns `'dev'` (no leading `./`), so `spawn` looks the program
// up on PATH, not in the folder. tag.yml's checker ran the wrong `./dev` (or none) from 2026-11-12 until run 8384; fixed in 774eea41c with `resolve`).
// A call of node:child_process `spawn`, `spawnSync`, `execFile`, `execFileSync` or `fork` whose program argument is a `path.join(..)` /
// `join(..)` (also `path.posix.join`, `path.win32.join`) with a RELATIVE string literal first part (`'.'`, `'..'`, `''`, `'./x'`, `'scripts'`, not
// `/x`, `C:\x`, `~`), or a variable whose `const`/`let`/`var` in the same file is such a join, is a finding. Use `resolve(..)` (absolute) or an
// absolute path. A first part that is an expression (`root`, `__dirname`, `process.cwd()`) is not judged: it is usually absolute.
// Read in every tracked .mjs/.cjs/.js/.mts/.cts/.ts file that mentions child_process, comments and string contents masked (`maskTs`, the
// one JavaScript masker). No parser or type checker: limits are a variable assigned in another file or by a later `=`, a join built from
// a spread or an alias of `join` (`import { join as j }`), and a program passed through a helper.
// ER-41: report-only (warnings) until its first green run on the real tree, recorded by run id here. Recorded: not yet.
import { warn } from '../finding.mjs';
import { maskTs } from './C71-test-timing.mjs';

export const SEVERITY = warn;
const SOURCE = /\.(?:mjs|cjs|js|mts|cts|ts)$/;
const CALL = /(?<![\w$])(?:(?:child_process|cp|childProcess)\s*\.\s*)?(spawn|spawnSync|execFile|execFileSync|fork)\s*\(/g;
const JOIN = /^\s*(?:await\s+)?(?:(?:node:)?path\s*\.\s*(?:posix\s*\.\s*|win32\s*\.\s*)?)?join\s*\(/;
const LITERAL = /^\s*(['"])(.*?)\1\s*(?:,|\))/s;

const lineOf = (text, index) => {
  let line = 1;
  for (let i = 0; i < index; i++) if (text.charCodeAt(i) === 10) line++;
  return line;
};

/** The `[start, end)` of the first argument of the call whose `(` is at `open` (masked `code` gives the structure). */
function firstArg(code, open) {
  let depth = 0;
  for (let i = open; i < code.length; i++) {
    const c = code[i];
    if (c === '(' || c === '[' || c === '{') depth++;
    else if (c === ')' || c === ']' || c === '}') {
      if (--depth === 0) return [open + 1, i];
    } else if (c === ',' && depth === 1) return [open + 1, i];
  }
  return [open + 1, code.length];
}

/** Whether `part` (a string literal's content) is a relative path: not absolute, not a drive, not `~`. */
export const isRelativePart = (part) => !/^(?:[\\/]|[A-Za-z]:|~)/.test(part);

/** Whether `expr` (original text) is a join call whose first part is a relative string literal. */
export function relativeJoin(expr) {
  const m = JOIN.exec(expr);
  if (!m) return false;
  const first = LITERAL.exec(expr.slice(m[0].length));
  return first !== null && isRelativePart(first[2]);
}

/** [{ line, message }] of the child_process calls in `text` whose program is a join with a relative first part. */
export function joinedProgramCalls(text) {
  if (!/child_process/.test(text)) return [];
  const code = maskTs(text);
  const out = [];
  for (const m of code.matchAll(CALL)) {
    if (/\bfunction\s*\*?\s*$/.test(code.slice(Math.max(0, m.index - 20), m.index))) continue; // a declaration, not a call
    const open = m.index + m[0].length - 1;
    const [a, b] = firstArg(code, open);
    const arg = text.slice(a, b);
    const ident = /^\s*([A-Za-z_$][\w$]*)\s*$/.exec(code.slice(a, b));
    let why = null;
    if (relativeJoin(arg)) why = `\`${arg.trim().replace(/\s+/g, ' ')}\``;
    else if (ident) {
      const assign = new RegExp(`(?<![\\w$.])(?:const|let|var)\\s+${ident[1].replace(/\$/g, '\\$')}\\s*=\\s*`, 'g');
      for (const d of code.matchAll(assign)) {
        const rest = text.slice(d.index + d[0].length, d.index + d[0].length + 400);
        if (relativeJoin(rest)) {
          why = `\`${ident[1]}\`, assigned on line ${lineOf(text, d.index)} from a join with a relative first part`;
          break;
        }
      }
    }
    if (why) out.push({ line: lineOf(text, m.index), call: m[1], message: `${m[1]}(..) takes the program ${why}: \`join('.', 'x')\` returns \`'x'\`, which is looked up on PATH, not in the folder` });
  }
  return out;
}

export default {
  id: 'C143',
  scope: 'root',
  mode: 'report-only',
  title: 'A spawned program is never a join with a relative first part',
  rule: 21,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      if (!SOURCE.test(f) || f.includes('node_modules/')) continue;
      const text = ctx.read(f);
      if (!text) continue;
      for (const hit of joinedProgramCalls(text)) {
        out.push(SEVERITY('C143', 21, f, hit.line, hit.message, 'Use `resolve(..)` (absolute, from the process folder) or an absolute path as the program; `join` never adds the leading `./`.'));
      }
    }
    return out;
  },
};
