// C127 (the number after C126; none of the open branches uses it on 2026-11-15): a recursive `rm` of a computed path in a tracked
// shell script goes through `safe_rm <root> <path>` (conformance/ci/safe-rm.sh), the shell copy of paddock-dev's `remove_under`.
// The class (ER-21): a computed delete that reached beyond its folder or nearly did: the 2026-10-31 scratchpad wipe, the
// 2026-11-10 live-worktree removal, the prune IFS near-miss and the prune stat fallback (docs/lessons.md).
// A site is `rm` at a command's start (also after `trap '`, `then`, `do`, `xargs` ...) with `-r`, `-R` or `--recursive` among
// its flags and a target holding `$` or a backtick: a bare variable, a guarded one (`"${X:?}"` stops an empty value, not a wrong
// one) or one below an owned prefix (`"$root/dist"`). C115 already refuses the bare unguarded variable (an error, with its
// baseline); this check is wider and only warns. Not a site: a literal path, a non-recursive `rm`, the script's own folder from
// `mktemp` (`tmp=$(mktemp -d)` ... `rm -rf "$tmp"`), full-line comments, and a site with `# conformance: computed-rm — <why>` (or
// C115's `# conformance: scoped-cleanup — <why>`) on its line or the line above. One finding per line.
// Read: `.sh` and `.bash` files and extensionless scripts with a shell shebang. Not read: tests (`*.test.sh`, `tests/`, `test/`),
// fixtures, research harnesses under a `docs/` folder, node_modules, build output, vendored code and the helper itself.
// Known gaps: an `rm` whose target is assembled in an array or read by `xargs`, `find -delete`, and a target the script reads
// from a file into a variable named like a mktemp one elsewhere in the file (names are collected per file, not per scope).
// ER-41: report-only (warnings) until its first green run on the real tree, recorded by run id in docs/gates.toml; then the
// sites left are migrated or waived and SEVERITY flips to error.
import { warn } from '../finding.mjs';

export const SEVERITY = warn;
/** This check's waiver, or C115's: a site already justified as scoped to its own items needs no second comment. */
const WAIVER = /conformance:\s*(?:computed-rm|scoped-cleanup)\s*[—–-]+\s*\S.{2,}/;
const SKIP_PATH =
  /(?:^|\/)(?:node_modules|dist|target|vendor)\/|(?:^|\/)(?:tests?|fixtures?)\/|\.test\.(?:ba)?sh$|(?:^|\/)docs\/|^conformance\/ci\/safe-rm\.sh$/;
const SHELL_FILE = /\.(?:ba)?sh$/;
/** `rm` where a command starts: line start, after `;`, `&`, `|`, `(`, `{`, a quote that opens a command string, or a keyword. */
const RM = /(?:^|[;&|({`'"]|\b(?:then|do|else|xargs|exec|command|sudo|time|nice)\s)[ \t]*rm[ \t]+([^\n;&|)#'`]*)/gm;
const FIX =
  'Remove it with `safe_rm <root> <path>` (`. conformance/ci/safe-rm.sh`): it refuses an empty path, `/`, a top-level or home root, `..`, a path outside the root and a link on the way. A site that is fine carries `# conformance: computed-rm — <why>`.';

/** Names the file assigns from `mktemp`: the script's own folders. */
const tempNames = (code) =>
  new Set([...code.matchAll(/(?:^|[\s;&(])(?:(?:local|export|declare|readonly)\s+)?(\w+)=["']?(?:\$\(|`)\s*mktemp\b/g)].map((m) => m[1]));

const unquote = (w) => w.replace(/^["']|["']$/g, '');

/** Whether a tracked file is read: a shell script that is not a test, fixture, record or the helper. */
export const isShellSource = (file, head = '') =>
  !SKIP_PATH.test(file) && (SHELL_FILE.test(file) || (!/\.[^/]+$/.test(file) && /^#!.*\b(?:ba|da|k|z)?sh\b/.test(head)));

/** The computed recursive deletes of one file: [{ line, text }]. */
export function computedRmSites(text, file = 'x.sh') {
  if (!isShellSource(file, text.slice(0, 80))) return [];
  const raw = text.split('\n');
  const code = raw.map((l) => (/^\s*#/.test(l) ? ' '.repeat(l.length) : l)).join('\n');
  const temps = tempNames(code);
  const out = [];
  for (const m of code.matchAll(RM)) {
    const words = m[1].split(/\s+/).filter(Boolean);
    const flags = [];
    const targets = [];
    let rest = false;
    for (const w of words) {
      if (!rest && w === '--') rest = true;
      else if (!rest && w.startsWith('-') && w.length > 1) flags.push(w);
      else if (!/^\d?>/.test(w)) targets.push(w);
    }
    const recursive = flags.some((w) => w === '--recursive' || /^-[a-zA-Z]*[rR][a-zA-Z]*$/.test(w));
    const computed = targets.filter((t) => /[$`]/.test(t)).filter((t) => {
      const v = /^\$(?:(\w+)|\{(\w+)\})$/.exec(unquote(t));
      return !(v && temps.has(v[1] ?? v[2]));
    });
    if (!recursive || computed.length === 0) continue;
    const line = code.slice(0, m.index + m[0].length).split('\n').length;
    if (WAIVER.test(raw[line - 1] ?? '') || WAIVER.test(raw[line - 2] ?? '') || out.some((s) => s.line === line)) continue;
    out.push({ line, text: (raw[line - 1] ?? '').trim() });
  }
  return out;
}

export default {
  id: 'C127',
  scope: 'root',
  mode: 'report-only',
  title: 'A recursive delete of a computed path in a shell script goes through safe_rm',
  rule: 2,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      if (SKIP_PATH.test(f)) continue;
      const text = ctx.read(f);
      if (!text || !/\brm\b/.test(text)) continue;
      for (const s of computedRmSites(text, f)) {
        out.push(SEVERITY('C127', 2, f, s.line, `a recursive rm of a computed path (\`${s.text.slice(0, 120)}\`) is not guarded`, FIX));
      }
    }
    return out;
  },
};
