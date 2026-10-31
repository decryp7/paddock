// C59: `read -t N` is not a sleep (rule 40, ER-21). When its input is /dev/zero or /dev/null, or a pipe from a command that
// ends at once (`: |`, `true |`, `echo |`, `yes |`, `cat /dev/zero |`), `read` returns immediately (data at once, or EOF), so a
// loop that "waits" with it spins a CPU core. Seen as `read -t 20 < /dev/zero` in an agent script and `read -t 1 _ </dev/null`
// in a test helper's embedded shell. Flagged, in tracked shell scripts (*.sh, *.bash, *.zsh), workflow and other YAML
// (`run:` blocks), and string literals in Rust, TypeScript and JavaScript files that build shell commands:
// - `read ... -t <n> ...` (n above 0; `-t 0` is a poll for input, not a wait) with `< /dev/zero`, `< /dev/null` or `0< ...`;
// - the same `read` at the end of a pipe from `:`, `true`, `false`, `echo`, `printf`, `yes` or `cat /dev/zero|null`.
// The conformance package's own tests (conformance/test/) hold the bad examples and are not scanned.
// Limits (documented, not checked): a `read -t` on an inherited stdin, a file, a here-string or a variable-built redirect is
// not flagged (a real fifo or a terminal prompt is fine), a `read` split over lines with `\` continuation is not joined, and
// comment lines (`#`, `//`, `*`) are skipped. The fix: `sleep N`, or `perl -e 'select(undef,undef,undef,N)'` where sleep
// is unavailable. A justified case carries `// conformance: busy-wait-read — <reason>` (or `#`) on the line or the line above.
import { error } from '../finding.mjs';

const WAIVER = /conformance:\s*busy-wait-read\s*[—–-]+\s*(\S.{2,})/;
const BARE_WAIVER = /conformance:\s*busy-wait-read/;
const FILES = /\.(?:sh|bash|zsh|ya?ml|rs|[cm]?[jt]s|tsx?)$/;
const COMMENT = /^\s*(?:#|\/\/|\*|\/\*)/;
/** `read` with a positive `-t` timeout among its options (`-t 1`, `-t1`, `-rt 2`, `-t 0.5`). */
const READ_T = /(?:^|[\s;&|(`"'{])read\s+(?:-[A-Za-z]*\s+)*?-[A-Za-z]*t\s*(\d+(?:\.\d+)?)(?=[\s"'`;)|&]|$)/;
const DEAD_INPUT = /(?:^|\s)\d?<\s*["']?\/dev\/(?:zero|null)\b/;
const DEAD_SOURCE = /(?:^|[\s;&(`"'{])(?:(?::|true|false|yes|echo|printf)(?:\s[^|;&]*)?|cat\s+\/dev\/(?:zero|null))\s*\|\s*$/;

/** The busy-wait reads in one line of text: [{ col }]. Shell lines are split into simple commands first. */
export function busyReadsInLine(line) {
  const out = [];
  // A literal `\n` inside a Rust/JS string ends a command like a newline does.
  const parts = line.split(/\\n|;|&&|\|\||\)|`/);
  let offset = 0;
  for (const part of parts) {
    const m = READ_T.exec(part);
    if (m && Number(m[1]) > 0) {
      const rest = part.slice(m.index + m[0].length);
      const before = line.slice(0, offset + m.index);
      const afterPipe = /\|\s*$/.test(part.slice(0, m.index + 1)) || /\|\s*$/.test(before.replace(/\s+$/, '') + ' ');
      if (DEAD_INPUT.test(rest)) out.push({ col: offset + m.index });
      else if (afterPipe && DEAD_SOURCE.test(before + ' ')) out.push({ col: offset + m.index });
    }
    offset += part.length + 1;
  }
  return out;
}

/** The busy-wait reads in a source: [{ line, waived: false | 'no-reason' | 'reason' }]. */
export function busyReads(text) {
  const lines = text.split('\n');
  const out = [];
  lines.forEach((l, i) => {
    if (COMMENT.test(l) || !/\bread\b/.test(l)) return;
    // `a | read -t 1` : the pipe source is on the same line, before the read.
    const hits = busyReadsInLine(l);
    if (hits.length === 0) return;
    const here = `${l}\n${lines[i - 1] ?? ''}`;
    const waived = WAIVER.test(here) ? 'reason' : BARE_WAIVER.test(here) ? 'no-reason' : false;
    out.push({ line: i + 1, waived });
  });
  return out;
}

export default {
  id: 'C59',
  title: '`read -t` is not used as a sleep',
  rule: 40,
  run(ctx) {
    const out = [];
    const fix = 'Use `sleep N` (or `perl -e \'select(undef,undef,undef,N)\'` where sleep is unavailable): `read -t N` on /dev/null, /dev/zero or an ended pipe returns at once and the loop spins a core. A justified case carries `// conformance: busy-wait-read — <reason>`.';
    for (const f of ctx.files()) {
      if (!FILES.test(f) || f.startsWith('conformance/test/')) continue; // the checks' own fixtures are the bad examples
      const text = ctx.read(f);
      if (!text || !/\bread\b/.test(text) || !/-\w*t\s*\d/.test(text)) continue;
      for (const p of busyReads(text)) {
        if (p.waived === 'reason') continue;
        if (p.waived === 'no-reason') out.push(error('C59', 40, f, p.line, 'the busy-wait-read waiver gives no reason', 'Write `conformance: busy-wait-read — <why the read may return at once>`.'));
        else out.push(error('C59', 40, f, p.line, '`read -t` on an input that is at EOF or always readable returns at once, so it is not a sleep and the loop spins a core', fix));
      }
    }
    return out;
  },
};
