// C115: a cleanup touches only its own items (ER-2, ER-14; the third sighting of "a cleanup that reaches beyond its own items":
// 2026-10-31 a reviewer's cleanup wiped the shared scratchpad, 2026-11-10 a worktree cleanup removed a live agent worktree,
// 2026-11-12 the pre-push sweep ran an unscoped `git worktree prune`). In tracked Rust, shell, PowerShell, JS/MJS and workflow
// YAML (also a Command argument array such as `["worktree", "prune"]`) it flags:
// - `git worktree prune`: it drops the records of every worktree whose folder is missing at that moment, also a live agent's on a
//   drive that is not mounted. Use `./dev worktree clean` (unlocked, merged, clean, not in use; never force);
// - `docker|podman system|container|volume|image|network|builder prune`, and a mass removal of containers by listing them
//   (`docker rm -f $(docker ps -aq)`, `docker ps -q | xargs docker stop`) unless the listing is narrowed by `--filter`: they reach
//   every container and image of the machine, also another agent's and the user's. Stop containers this task started, by name or label;
// - `git clean` with `-x`, `-X` or `-d` and no pathspec (it runs on the whole checkout, tracked-looking build and agent folders
//   included). A dry run (`-n`, `--dry-run`) and a command with a pathspec are fine;
// - `rm -r` / `Remove-Item -Recurse` whose only target is a bare variable (`"$VAR"`, `$VAR/*`, `$env:X`), `*`, `./*` or `/*`.
//   Heuristic, kept narrow on purpose: a target with a literal part (`"$DIR/build"`) is not flagged, because the variable is then
//   a prefix the script owns; an empty or wrong variable in a bare target deletes the parent. Rust `remove_dir_all` and JS
//   `rmSync` are not seen (too many owned temp folders; their callers hold the path they created).
// Not read: comments (full-line `//`, `///`, `#`), Markdown and other docs, fixtures, check tests and the guard hook's own rule
// files (`tools/paddock-dev/src/hook/`, which name the patterns to refuse them).
// Existing sites (the `rm -r` on a bare variable of scripts that clean up their own `mktemp` folder) sit in
// `conformance/baselines/C115.json` as {file, hash of the normalised line}, as C71's do: a listed site passes, any other is an error.
// The baseline may only shrink (a listed site that is gone is a warning; an entry the merge base's baseline lacks is an error).
// A site that only touches its own items carries `// conformance: scoped-cleanup — <why>` (or `# conformance: ...`) on the line or the
// line above.
import { error, warn } from '../finding.mjs';
import { againstBaseline, baselineGrowth, lineHash } from './C71-test-timing.mjs';

export const BASELINE_PATH = 'conformance/baselines/C115.json';

const WAIVER = /conformance:\s*scoped-cleanup\s*[—–-]+\s*(\S.{2,})/;
const BARE_WAIVER = /conformance:\s*scoped-cleanup/;
const SOURCE = /\.(?:rs|sh|bash|ps1|psm1|js|mjs|cjs|ya?ml)$/;
const SKIP_PATH = /^conformance\/(?:test|src\/checks\/C115)|(?:^|\/)(?:node_modules|dist|target|fixtures|vendor)\/|^docs\/|\.md$|^tools\/paddock-dev\/src\/hook\//;
const Q = '["\'`]';
const ENGINE = '(?:docker|podman)';
const KINDS = '(?:system|container|volume|image|network|builder|buildx)';
const STOP = '(?:rm|stop|kill|restart)';

const PRUNE_FIX = 'Use `./dev worktree clean` (it removes only unlocked, merged, clean worktrees that are not in use). A site that only touches its own items carries `// conformance: scoped-cleanup — <why>`.';
const DOCKER_FIX = 'Stop or remove only containers this task started, by name or label (`docker rm -f <name>`, `docker ps -q --filter label=<own>`), never every container or image on the machine. A justified case carries `// conformance: scoped-cleanup — <why>`.';
const CLEAN_FIX = 'Give `git clean` a pathspec of the items you own (`git clean -fd -- <path>`), or preview with `-n`. A justified case carries `// conformance: scoped-cleanup — <why>`.';
const RM_FIX = 'Delete a path you built from an owned prefix (`rm -rf "$OWN_DIR/build"`), or a folder you just created with `mktemp -d` and say so: `// conformance: scoped-cleanup — <why it only holds this task\'s files>`. Never a bare variable, `*` or `/*`.';

const RULES = [
  { re: new RegExp(`\\bworktree(?:${Q}?\\s*,\\s*${Q}|\\s+)prune\\b`, 'g'), what: '`git worktree prune` drops the record of every worktree whose folder is missing right now, also a live agent\'s', fix: PRUNE_FIX },
  { re: new RegExp(`\\b${ENGINE}\\s+${KINDS}\\s+prune\\b`, 'g'), what: 'a container-engine `prune` removes every stopped container, image or volume on the machine, not only this task\'s', fix: DOCKER_FIX },
  { re: new RegExp(`\\b${KINDS}${Q}\\s*,\\s*${Q}prune\\b`, 'g'), needs: new RegExp(`\\b${ENGINE}\\b`), what: 'a container-engine `prune` removes every stopped container, image or volume on the machine, not only this task\'s', fix: DOCKER_FIX },
  {
    re: new RegExp(`\\b${ENGINE}\\s+(?:container\\s+)?${STOP}\\b[^\\n]*(?:\\$\\(|\`)\\s*${ENGINE}\\s+(?:container\\s+)?(?:ps|ls)\\b[^\\n)\`]*|\\b${ENGINE}\\s+(?:container\\s+)?(?:ps|ls)\\b[^\\n|]*\\|\\s*xargs\\s+(?:-\\S+\\s+)*${ENGINE}\\s+(?:container\\s+)?${STOP}\\b`, 'g'),
    reject: /--filter/,
    what: 'a mass removal of every listed container reaches containers of other agents and of the user',
    fix: DOCKER_FIX,
  },
];

const lineAt = (text, index) => text.slice(0, index).split('\n').length;

/** Whether `flags` (words like `-fdx`, `--dry-run`) clean untracked directories or ignored files, and are not a dry run. */
function cleansWide(flags) {
  const words = flags.split(/\s+/).filter(Boolean);
  if (words.some((w) => w === '--dry-run' || /^-[a-zA-Z]*n[a-zA-Z]*$/.test(w))) return false;
  return words.some((w) => /^-[a-zA-Z]*[xXd][a-zA-Z]*$/.test(w));
}

/** `text` with the full-line comments blanked (same length, newlines kept). */
function maskComments(text, file) {
  const slash = /\.(?:rs|js|mjs|cjs)$/.test(file);
  return text
    .split('\n')
    .map((l) => ((slash ? /^\s*\/\//.test(l) : /^\s*#/.test(l)) ? ' '.repeat(l.length) : l))
    .join('\n');
}

/** The unscoped cleanups of one file: [{ line, what, fix, waived: 'reason' | 'no-reason' | undefined }]. */
export function unscopedCleanups(text, file = 'x.sh') {
  const code = maskComments(text, file);
  const lines = text.split('\n');
  const hits = [];
  for (const r of RULES) {
    for (const m of code.matchAll(r.re)) {
      if (r.reject?.test(m[0])) continue;
      if (r.needs && !r.needs.test(code.slice(Math.max(0, m.index - 160), m.index))) continue;
      hits.push({ index: m.index, what: r.what, fix: r.fix });
    }
  }
  // git clean, as a command line: `git [opts] clean <flags> [path...]` up to the end of the command.
  for (const m of code.matchAll(/\bgit\b[^\n;|&]*?\bclean\b[ \t]+((?:-{1,2}[\w-]+[ \t]*)+)([^\n;|&)]*)/g)) {
    if (cleansWide(m[1]) && !/^\s*(?:--(?:\s|$)|[^\s-])/.test(m[2])) hits.push({ index: m.index, what: '`git clean` with `-d`, `-x` or `-X` and no pathspec removes files across the whole checkout, also other tasks\' folders', fix: CLEAN_FIX });
  }
  // git clean, as an argument array: ["clean", "-fdx"] with no path element before the closing bracket.
  for (const m of code.matchAll(new RegExp(`${Q}clean${Q}\\s*,\\s*((?:${Q}-[^"'\`\\n]*${Q}\\s*,?\\s*)+)\\]`, 'g'))) {
    if (cleansWide(m[1].replace(/["'`,]/g, ' '))) hits.push({ index: m.index, what: '`git clean` with `-d`, `-x` or `-X` and no pathspec removes files across the whole checkout, also other tasks\' folders', fix: CLEAN_FIX });
  }
  // rm -r with a bare variable or glob as its only target.
  const bare = '(?:\\$(?:\\w+|\\{[^}\\n]+\\})|\\*|\\./\\*|/\\*)';
  for (const m of code.matchAll(new RegExp(`\\brm[ \\t]+((?:-[a-zA-Z-]+[ \\t]+)+)(["']?)${bare}(?:/\\*)?\\2(?:/\\*)?(?=[ \\t]*(?:$|[;&|)#]|\\n))`, 'gm'))) {
    if (/^-[a-zA-Z]*[rR]|--recursive/.test(m[1].trim().split(/\s+/).find((w) => /[rR]|--recursive/.test(w)) ?? '')) {
      hits.push({ index: m.index, what: '`rm -r` on a bare variable or glob deletes whatever the variable holds, an empty one the parent', fix: RM_FIX });
    }
  }
  const psBare = /^"?(?:\$\w+(?::\w+)?|\$\{[^}]+\}|\*)(?:[\\/]\*)?"?$/;
  for (const m of code.matchAll(/\bRemove-Item\b[^\n;|]*/gi)) {
    const words = m[0].split(/\s+/).slice(1).filter(Boolean);
    if (!words.some((w) => /^-Recurse$/i.test(w))) continue;
    // The target is the -Path / -LiteralPath value or the first positional word; -Filter, -Include and -Exclude take a value.
    let target;
    for (let k = 0; k < words.length && target === undefined; k++) {
      if (/^-(?:Path|LiteralPath)$/i.test(words[k])) target = words[k + 1] ?? '';
      else if (/^-(?:Filter|Include|Exclude|Credential|Stream)$/i.test(words[k])) k++;
      else if (!words[k].startsWith('-') && !/^#/.test(words[k])) target = words[k];
    }
    if (target !== undefined && psBare.test(target)) hits.push({ index: m.index, what: '`Remove-Item -Recurse` on a bare variable or glob deletes whatever the variable holds', fix: RM_FIX });
  }
  return hits
    .sort((a, b) => a.index - b.index)
    .map(({ index, what, fix }) => {
      const line = lineAt(code, index);
      const near = [lines[line - 1] ?? '', lines[line - 2] ?? ''].join('\n');
      const waived = WAIVER.test(near) ? 'reason' : BARE_WAIVER.test(near) ? 'no-reason' : undefined;
      return { line, what, fix, waived };
    });
}

/** Whether a tracked file is read by C115: its extension, or an extensionless script with a shell shebang. */
export const isCleanupSource = (file, head = '') => !SKIP_PATH.test(file) && (SOURCE.test(file) || (!/\.[^/]+$/.test(file) && /^#!.*\b(?:ba|z)?sh\b/.test(head)));

export default {
  id: 'C115',
  scope: 'both',
  mode: 'gate',
  title: 'A cleanup touches only its own items',
  rule: 2,
  run(ctx) {
    const out = [];
    const sites = [];
    for (const f of ctx.files()) {
      if (SKIP_PATH.test(f)) continue;
      const text = ctx.read(f);
      if (!text || !isCleanupSource(f, text.slice(0, 80))) continue;
      if (!/prune|\bclean\b|\brm\b|Remove-Item|docker|podman/.test(text)) continue;
      const lines = text.split('\n');
      for (const k of unscopedCleanups(text, f)) {
        if (k.waived === 'reason') continue;
        if (k.waived === 'no-reason') out.push(error('C115', 2, f, k.line, 'the scoped-cleanup waiver gives no reason', 'Write `// conformance: scoped-cleanup — <why it only touches its own items>`.'));
        else sites.push({ file: f, line: k.line, what: k.what, fix: k.fix, text: lines[k.line - 1] ?? '', hash: lineHash(lines[k.line - 1] ?? '') });
      }
    }
    let baseline = { sites: [] };
    const raw = ctx.read(BASELINE_PATH);
    if (raw) baseline = JSON.parse(raw);
    const { fresh, stale } = againstBaseline(sites, baseline.sites);
    for (const s of fresh) out.push(error('C115', 2, s.file, s.line, `${s.what} (ER-2, ER-14): ${s.text.trim().slice(0, 100)}`, s.fix));
    for (const e of stale) out.push(warn('C115', 2, e.file, e.line ?? 1, `${BASELINE_PATH} lists a site that is gone (hash ${e.hash}); the baseline may only shrink`, 'Remove the entry from the baseline.'));
    if (baseline.sites.length) {
      const g = baselineGrowth(ctx, baseline.sites, BASELINE_PATH);
      if (g.reason) out.push(warn('C115', 2, BASELINE_PATH, 1, `the baseline's shrink-only rule is not enforced here: ${g.reason}`, 'Fetch the target branch (`git fetch origin main`, full history), or set PADDOCK_BASE_REF to the ref to compare with.'));
      else if (g.added.length) out.push(error('C115', 2, BASELINE_PATH, 1, `the baseline grew: ${g.added.length} entr${g.added.length === 1 ? 'y is' : 'ies are'} not in the baseline at the merge base ${g.base.slice(0, 9)}; it may only shrink`, 'Remove the entries and scope the cleanups (an owned prefix, or `// conformance: scoped-cleanup — <why>`). Growing the baseline needs the user\'s approval.'));
    }
    return out;
  },
};
