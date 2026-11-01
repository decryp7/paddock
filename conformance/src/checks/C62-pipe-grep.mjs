// C62: no quiet grep fed by a pipe where pipefail is on (rule 40: a recurring bug gets a check). `cmd | grep -q x` under
// `set -o pipefail` fails although x matched whenever grep exits at its match before the writer has written everything: the
// writer gets SIGPIPE and the pipeline takes its status. It said "DMG is not signed" for a signed DMG (the shell demo, C04)
// and read an installed rustup component as missing (setup-rust, 2026-10-31). The fix is to match without a pipe:
// `grep -q x <<<"$out"` (or `<<<"$(cmd)"`), or `[ -n "$(find ...)" ]` for "anything found".
//
// Where pipefail is on: a shell script (`.sh`, `.bash`) that runs `set -o pipefail` (also `set -euo pipefail`, `set -e -o
// pipefail`); a `run:` step of a workflow or composite action under .gitea/ or .github/ with `shell: bash` (the runner then
// uses `bash --noprofile --norc -eo pipefail {0}`, GitHub's documented default for an explicit bash, which Gitea's act
// follows), whose workflow sets `defaults: run: shell: bash`, or whose script sets pipefail itself. A step with no shell
// runs `bash -e {0}` (no pipefail) and is not flagged.
// What is flagged: grep, egrep or fgrep with -q (in any option cluster: -q, -Eq, -qx, -Fxq), --quiet or --silent, and
// `head` (any options), right after a single `|`, outside comments. `head` quits after its lines like grep -q after its
// match (`tail -n +N | head -n 3` died with 141 before its ::error::, 2026-11-01); use `sed -n 'A,Bp' file`, or capture. An error: every site is fixed (36 in demo, app and CI scripts when it moved to main with head).
// A justified case carries `# conformance: pipe-grep — <reason>` on the line or the line above; a bare marker does not count.
import { error } from '../finding.mjs';
import { parseSteps } from '../util/workflows.mjs';

const WAIVER = /conformance:\s*pipe-grep\s*[—–-]+\s*\S.{2,}/;
const PIPEFAIL = /^\s*set\s+(?:[-+][A-Za-z]+\s+)*-[A-Za-z]*o\s+pipefail\b/m;
const WORKFLOW_FILE = /(?:^|\/)\.(?:gitea|github)\/(?:workflows\/[^/]+\.ya?ml|actions\/[^/]+\/action\.ya?ml)$/;
const DEFAULT_BASH = /^\s*defaults:\s*\n\s+run:\s*\n(?:\s+[\w-]+:.*\n)*?\s+shell:\s*bash\s*$/m;
const QUIET = /^(?:-[A-Za-z]*q[A-Za-z]*|--quiet|--silent)$/;

/** Whether a shell script turns pipefail on. */
export function setsPipefail(text) {
  return PIPEFAIL.test(text);
}

/** The code part of one shell line: up to an unquoted `#` that starts a word (a comment), with quoted text blanked. */
export function codeOf(line) {
  let out = '';
  let quote = '';
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (quote) {
      if (c === quote) {
        quote = '';
        out += c;
      } else if (c === '\\' && quote === '"') {
        out += '  ';
        i++;
      } else out += ' ';
      continue;
    }
    if (c === '\\') {
      out += '  ';
      i++;
      continue;
    }
    if (c === "'" || c === '"') {
      quote = c;
      out += c;
      continue;
    }
    if (c === '#' && (i === 0 || /\s/.test(line[i - 1]))) break;
    out += c;
  }
  return out;
}

/** Whether one shell line feeds a quiet grep through a pipe. */
export function pipesIntoQuietGrep(line) {
  const code = codeOf(line);
  for (const m of code.matchAll(/(?:^|[^|])\|(?![|&])\s*(?:[A-Za-z_][A-Za-z0-9_]*=\S*\s+)*(?:command\s+)?(?:[ef]?grep(?=\s)([^|;&)]*)|head(?=\s|$))/g)) {
    if (m[1] === undefined) return true; // a piped head quits after its lines, whatever the writer still has to say
    if (m[1].trim().split(/\s+/).some((tok) => QUIET.test(tok))) return true;
  }
  return false;
}

/** 1-based lines (offset by `first - 1`) of `lines` that pipe into a quiet grep and carry no waiver. */
export function quietGrepLines(lines, first = 1) {
  const out = [];
  lines.forEach((l, i) => {
    if (!pipesIntoQuietGrep(l)) return;
    if (WAIVER.test(l) || WAIVER.test(lines[i - 1] ?? '')) return;
    out.push(first + i);
  });
  return out;
}

/** The lines of a step's `run:` script: [{ line (1-based in the file), text }]. */
function runLines(step) {
  const lines = step.text.split('\n');
  const at = lines.findIndex((l) => /^\s*(?:-\s+)?run:/.test(l));
  if (at < 0) return [];
  const head = lines[at].replace(/^\s*(?:-\s+)?run:\s*/, '');
  if (!/^[|>][+-]?\s*$/.test(head)) return [{ line: step.line + at, text: head }];
  const base = lines[at].length - lines[at].trimStart().length;
  const out = [];
  for (let j = at + 1; j < lines.length; j++) {
    const l = lines[j];
    if (l.trim() !== '' && l.length - l.trimStart().length <= base) break;
    out.push({ line: step.line + j, text: l });
  }
  return out;
}

/** Findings for a workflow or action file: [{ line }] of pipe-fed quiet greps in bash steps that run with pipefail. */
export function workflowSites(text) {
  const defaultBash = DEFAULT_BASH.test(text);
  const out = [];
  for (const step of parseSteps(text)) {
    if (!step.run) continue;
    const bash = /^\s*shell:\s*bash\s*$/m.test(step.text) || (defaultBash && !/^\s*shell:/m.test(step.text));
    if (!bash && !setsPipefail(step.run)) continue;
    const lines = runLines(step);
    lines.forEach((l, i) => {
      if (!pipesIntoQuietGrep(l.text)) return;
      if (WAIVER.test(l.text) || WAIVER.test(lines[i - 1]?.text ?? '')) return;
      out.push(l.line);
    });
  }
  return out;
}

const MESSAGE = '`… | grep -q` or `… | head` with pipefail on: the reader exits early, the writer can get SIGPIPE, and the pipeline then fails (status 141) although nothing went wrong';
const FIX = 'Match without a pipe: `grep -q PATTERN <<<"$out"` (capture the output first) or `<<<"$(cmd)"`; for the first lines of a file `sed -n 1,3p file`; for "anything found" use `[ -n "$(find …)" ]`. A justified case carries `# conformance: pipe-grep — <reason>`.';

export default {
  id: 'C62',
  title: 'No quiet grep or head fed by a pipe under pipefail',
  rule: 40,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      if (/\.(?:sh|bash)$/.test(f)) {
        const text = ctx.read(f);
        if (!text || !/grep/.test(text) || !setsPipefail(text)) continue;
        for (const line of quietGrepLines(text.split('\n'))) out.push(error('C62', 40, f, line, MESSAGE, FIX));
      } else if (WORKFLOW_FILE.test(f)) {
        const text = ctx.read(f);
        if (!text || !/grep/.test(text)) continue;
        for (const line of workflowSites(text)) out.push(error('C62', 40, f, line, MESSAGE, FIX));
      }
    }
    return out;
  },
};
