// C60: `read -t N` is not a sleep (rule 40, ER-21). When its input is /dev/zero or /dev/null, a here-string or heredoc, a
// closed stdin (`<&-`), a trivial process substitution (`< <(:)`), or a pipe from a command that ends at once (`:`, `true`,
// `echo`, `printf`, `yes`, `cat /dev/zero`), `read` returns immediately (data at once, or EOF), so a loop that "waits" with it
// spins a CPU core. Seen as `read -t 20 < /dev/zero` in an agent script and `read -t 1 _ </dev/null` in a test helper's
// embedded shell. Checked in every file the walk sees (tracked or not): shell scripts (*.sh, *.bash, *.zsh, and files without
// an extension that start with a `#!` line naming a shell, such as the root `dev`), YAML (workflow `run:` blocks), and the
// string literals of Rust, TypeScript and JavaScript files and package.json that build shell commands. Flagged:
// - `read` with a timeout option `-t <n>` anywhere among its options (`-s -n 1 -t 20`, `-p "x" -t 5`, `-d "" -t 5`, `-rt 2`,
//   `-t5`, `-t 0.5`), n above 0 (`-t 0` is a poll for input, not a wait); a variable timeout (`-t "$T"`, `-t ${DELAY:-5}`)
//   counts as a wait;
// - when that command's stdin is one of the sources above (`<> /dev/null` too; with `-u N`, fd N redirected as `N</dev/zero`), with or without a space before the `<`; `2</dev/null` is not stdin.
// Simple commands are split at `;`, `&&`, `||`, a single `&`, a single `|` and `)` (in Rust/TS/JS strings also at a literal
// `\n`), so a later command's redirect is not charged to the read; shell lines are cut at ` #`.
// Limits (documented, not checked): a `read -t` on an inherited stdin, a file, a fifo or a variable-built redirect is not
// flagged (a real fifo or a terminal prompt is fine); a `read` split over lines with `\` continuation is not joined; a pipe
// whose source is on an earlier line is not seen; a source other than the listed trivial ones is not judged; a group redirect (`{ …; } </dev/null`) and a quoted `|` before `-t` are not followed; `while read -t N` fed by a pipe is not flagged (the loop ends with its input); comment lines
// are skipped (`#` in shell and YAML; `//`, `/*`, `*` in code). The fix: `sleep N`, or `perl -e 'select(undef,undef,undef,N)'`
// where sleep is unavailable. A justified case carries `# conformance: busy-wait-read — <reason>` (shell, YAML) or
// `// conformance: busy-wait-read — <reason>` (Rust, TS, JS) on the line or the line above. The conformance package's own
// tests (conformance/test/) hold the bad examples and are not scanned.
import { error } from '../finding.mjs';

const WAIVER = /conformance:\s*busy-wait-read\s*[—–-]+\s*(\S.{2,})/;
const BARE_WAIVER = /conformance:\s*busy-wait-read/;
const SHELL_EXT = /\.(?:sh|bash|zsh|ya?ml)$/;
const CODE_EXT = /\.(?:[cm]?[jt]s|tsx?|rs)$|(?:^|\/)package\.json$/;
const SHEBANG = /^#!.*\b(?:ba|z|da|k|a)?sh\b/;
/** `read` with a timeout among its options: group 1 a literal number, group 2 a variable. */
const READ_T = /(?:^|[\s(`"'{])read\s+(?:\S+\s+)*?-[A-Za-z]*t\s*(?:(\d+(?:\.\d+)?)|(["']?\$(?:\{[^}\s]*\}|\w+)["']?))(?=[\s"'`;)|&<]|$)/;
const STDIN = '(?:^|[^\\d<>&]|(?<=\\S)\\d)0?'; // not an fd redirect such as 2<, but a digit ending a word (-t5) is fine
/** A stdin that returns at once: /dev/zero|null, a here-string or heredoc, a closed stdin, a trivial process substitution. */
const DEAD_INPUT = new RegExp(
  `${STDIN}<>?(?![<&(])\\s*["']?\\/dev\\/(?:zero|null)\\b|${STDIN}<<|${STDIN}<&-|${STDIN}<\\s*<\\(\\s*(?::|true|false|echo|printf)(?=[\\s)]|$)`,
);
/** A command that ends at once, as the left side of a pipe. */
const DEAD_SOURCE = /^\s*(?:do\s+|then\s+|\{\s*|\(\s*)?(?:(?::|true|false|yes|echo|printf)(?=\s|$)|cat\s+\/dev\/(?:zero|null)\b)/;

/** `line` split into simple commands: [{ text, pipe }] where `pipe` is the text of the command piped into this one, if any. */
export function commands(line, code) {
  const out = [];
  let cur = '';
  let prev = null;
  let viaPipe = false;
  const cut = (pipe) => {
    out.push({ text: cur, pipe: viaPipe ? prev : null });
    prev = cur;
    cur = '';
    viaPipe = pipe;
  };
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    const n = line[i + 1];
    if (code && c === '\\' && n === 'n') { cut(false); i++; }
    else if (c === '|' && n === '|') { cut(false); i++; }
    else if (c === '&' && n === '&') { cut(false); i++; }
    else if (c === '|') cut(true);
    else if (c === ';' || c === ')' || c === '`') cut(false);
    else if (c === '&' && !'<>'.includes(line[i - 1] ?? ' ') && n !== '>') cut(false);
    else cur += c;
  }
  cut(false);
  return out;
}

/** The busy-wait reads in a source: [{ line, waived: false | 'no-reason' | 'reason' }]. `kind` is 'shell' (default) or 'code'. */
export function busyReads(text, kind = 'shell') {
  const code = kind === 'code';
  const comment = code ? /^\s*(?:\/\/|\/\*|\*(?:\s|\/|$))/ : /^\s*#/;
  const lines = text.split('\n');
  const out = [];
  lines.forEach((raw, i) => {
    if (comment.test(raw) || !/\bread\b/.test(raw)) return;
    const l = code ? raw : raw.replace(/\s#.*$/, '');
    const hit = commands(l, code).some((c) => {
      const m = READ_T.exec(c.text);
      if (!m || (m[1] !== undefined && Number(m[1]) === 0)) return false;
      if (DEAD_INPUT.test(c.text)) return true;
      // `-u N` reads fd N: dead when that fd is redirected from a dead file (`3</dev/zero`).
      const fd = /\s-[A-Za-z]*u\s*(\d+)/.exec(c.text);
      if (fd && new RegExp(`(?:^|\\s)${fd[1]}<>?\\s*["']?\\/dev\\/(?:zero|null)\\b`).test(c.text)) return true;
      // A `while`/`until read` loop fed by a pipe ends with its input, so it is not a busy wait.
      return c.pipe !== null && DEAD_SOURCE.test(c.pipe) && !/^\s*(?:while|until)\s/.test(c.text);
    });
    if (!hit) return;
    const here = `${raw}\n${lines[i - 1] ?? ''}`;
    const waived = WAIVER.test(here) ? 'reason' : BARE_WAIVER.test(here) ? 'no-reason' : false;
    out.push({ line: i + 1, waived });
  });
  return out;
}

/** 'shell', 'code' or null for a file path and its text. */
export function kindOf(file, text) {
  if (SHELL_EXT.test(file)) return 'shell';
  if (CODE_EXT.test(file)) return 'code';
  const base = file.slice(file.lastIndexOf('/') + 1);
  if (!base.includes('.') && SHEBANG.test(text.slice(0, text.indexOf('\n') < 0 ? 200 : text.indexOf('\n')))) return 'shell';
  return null;
}

export default {
  id: 'C60',
  title: '`read -t` is not used as a sleep',
  rule: 40,
  run(ctx) {
    const out = [];
    const fix =
      'Use `sleep N` (or `perl -e \'select(undef,undef,undef,N)\'` where sleep is unavailable): `read -t N` on /dev/null, /dev/zero, a here-string or an ended pipe returns at once and the loop spins a core. A justified case carries `# conformance: busy-wait-read — <reason>` (shell, YAML) or `// conformance: busy-wait-read — <reason>` (Rust, TS, JS).';
    for (const f of ctx.files()) {
      if (f.startsWith('conformance/test/')) continue; // the checks' own fixtures are the bad examples
      const text = ctx.read(f);
      if (!text || !/\bread\b[^\n]*-\w*t/.test(text)) continue;
      const kind = kindOf(f, text);
      if (!kind) continue;
      for (const p of busyReads(text, kind)) {
        if (p.waived === 'reason') continue;
        if (p.waived === 'no-reason') out.push(error('C60', 40, f, p.line, 'the busy-wait-read waiver gives no reason', 'Write `conformance: busy-wait-read — <why the read may return at once>`.'));
        else out.push(error('C60', 40, f, p.line, '`read -t` on an input that is at EOF or always readable returns at once, so it is not a sleep and the loop spins a core', fix));
      }
    }
    return out;
  },
};
