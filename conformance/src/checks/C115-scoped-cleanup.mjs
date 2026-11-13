// C115: a cleanup touches only its own items (ER-2, ER-14; the third sighting of "a cleanup that reaches beyond its own items":
// 2026-10-31 a reviewer's cleanup wiped the shared scratchpad, 2026-11-10 a worktree cleanup removed a live agent worktree,
// 2026-11-12 the pre-push sweep ran an unscoped `git worktree prune`). In tracked Rust, shell, PowerShell, JS/MJS and workflow
// YAML it flags:
// - `git worktree prune` (a dry run, `-n` or `--dry-run`, is fine): it drops the record of every worktree whose folder is missing at
//   that moment, also a live agent's on a drive that is not mounted. And `git worktree remove --force`/`-f` inside a loop or under
//   `xargs` (the 2026-11-10 shape: one loop that forces away every worktree it lists). Use `./dev worktree clean`;
// - `docker|podman system|container|volume|image|network|builder|buildx prune`, and a mass removal of every listed object
//   (`docker rm -f $(docker ps -aq)`, `docker volume rm $(docker volume ls -q)`, `docker rmi $(docker images -q)`,
//   `docker ps -q | xargs docker stop`) unless the listing is narrowed by `-f` or `--filter` (also `--filter=`): they reach
//   every container, image and volume of the machine, also another agent's and the user's. Stop containers this task started, by
//   name or label;
// - `git clean` with `-x`, `-X` or `-d` and no pathspec (a pathspec of `.`, `./` or `:/` is the whole checkout, and `-e`/`--exclude`
//   values are not pathspecs). A dry run is fine;
// - `rm -r` (also `-R`, `--recursive`, with other flags in any order) / `Remove-Item -Recurse` with any target that is a bare variable
//   (`"$VAR"`, `$VAR/*`, `$env:X`), `~`, `~/*`, `.`, `*`, `./*` or `/*`. Heuristic, kept narrow on purpose: a target with a literal
//   part (`"$DIR/build"`) is not flagged, because the variable is then a prefix the script owns; `"${VAR:?}"` is guarded (it
//   fails when the variable is empty) and a variable assigned from `$(mktemp ...)` or from a value with a literal path part (`stage="$dir/setup-disc"`) in the same file is the script's own folder.
// In Rust and JS only commands are read: an argument array (`["worktree", "prune"]`) or a line that runs a command (`Command::new`,
// `.arg(`, `exec`, `spawn`, `system`, `sh -c`), never a message string that mentions one. `rm -r` is read in shell, YAML and PowerShell
// only. Accepted gaps: `xargs rm -rf`, `find <root> -delete`, and a command assembled at run time; Rust `remove_dir_all` and JS `rmSync`.
// The guard hook (`tools/paddock-dev/src/hook/rules/no_unscoped_cleanup.rs`) refuses the worktree and container shapes at run time; both
// answer the rows of `tools/paddock-dev/tests/fixtures/cleanup-corpus.tsv` the same way (each tests against it).
// Not read: full-line comments, the hook rule file, the guard hooks' deny-case tests (`.claude/hooks/*.test.sh`: commands fed to a
// hook as data, never run), this check, `conformance/test` and `conformance/fixtures`, every `tests/fixtures/` folder (sample scripts, the paddock-dev oracle files), `node_modules`, build output and vendored code.
// Known heuristic: the names that count as the script's own (mktemp, literal-part values) are collected over the whole file, not by
// scope or order, so `x="$A/b"; ...; x="$1"; rm -rf "$x"` passes. Keep a variable that is cleaned up assigned once.
// A site that only touches its own items carries `// conformance: scoped-cleanup — <why>` (or the `#` form) on the line or the line
// above; the check lists the waived sites in one warning. Existing sites (the `rm -r` on a bare variable of scripts that clean up their
// own folders) sit in `conformance/baselines/C115.json` as {file, hash of the normalised line}, as C71's do: a listed site passes, any
// other is an error. The baseline may only shrink (a listed site that is gone is a warning; an entry the merge base's baseline lacks is
// an error); `node conformance/bin/paddock-cleanup-baseline.mjs` lists the sites, `--shrink` removes stale entries.
import { error, warn } from '../finding.mjs';
import { againstBaseline, baselineGrowth, growthMovedWarning, lineHash, movedWarning, sourceGone } from './C71-test-timing.mjs';

export const BASELINE_PATH = 'conformance/baselines/C115.json';
const WAIVER = /conformance:\s*scoped-cleanup\s*[—–-]+\s*(\S.{2,})/;
const BARE_WAIVER = /conformance:\s*scoped-cleanup/;
const SOURCE = /\.(?:rs|sh|bash|ps1|psm1|js|mjs|cjs|ya?ml)$/;
const CODE = /\.(?:rs|js|mjs|cjs)$/;
const SKIP_PATH =
  /^conformance\/(?:test\/|fixtures\/|src\/checks\/C115)|(?:^|\/)(?:node_modules|dist|target)\/|(?:^|\/)tests\/fixtures\/|^apps\/[^/]+\/vendor\/|^tools\/paddock-dev\/(?:tests\/fixtures\/|src\/hook\/rules\/no_unscoped_cleanup\.rs$)|^\.claude\/hooks\/[^/]+\.test\.sh$/;
const Q = '["\'`]';
const ENGINE = '(?:docker|podman)';
const KIND = '(?:(?:container|volume|image|network)\\s+)?';
const KINDS = '(?:system|container|volume|image|network|builder|buildx)';
const STOP = '(?:rm|rmi|stop|kill|restart)';
const LIST = '(?:ps|ls|images|list)';
/** A line that runs a command in Rust or JS (as opposed to a message string). */
const RUNS = /\bCommand::new\b|\b(?:exec(?:Sync|File|FileSync)?|spawn(?:Sync)?|system)\s*\(|\.args?\(/;
/** The line with the contents of its string literals blanked, so a word inside a message is not read as a call. */
const unstring = (line) => line.replace(/"(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*'|`[^`]*`/g, (q) => q[0] + q[0]);

const PRUNE_FIX = 'Use `./dev worktree clean` (it removes only unlocked, merged, clean worktrees that are not in use). A site that only touches its own items carries `// conformance: scoped-cleanup — <why>`.';
const DOCKER_FIX = 'Stop or remove only objects this task started, by name or label (`docker rm -f <name>`, `docker ps -q --filter label=<own>`), never every container, image or volume on the machine. A justified case carries `// conformance: scoped-cleanup — <why>`.';
const CLEAN_FIX = 'Give `git clean` a pathspec of the items you own (`git clean -fd -- <path>`; `.` is the whole checkout), or preview with `-n`. A justified case carries `// conformance: scoped-cleanup — <why>`.';
const RM_FIX = 'Delete a path built from an owned prefix (`rm -rf "$OWN_DIR/build"`), a guarded variable (`rm -rf "${DIR:?}"`), or a folder this script made with `mktemp -d`. Never a bare unguarded variable, `~`, `.`, `*` or `/*`; otherwise `// conformance: scoped-cleanup — <why it only holds this task\'s files>`.';

const WHAT = {
  prune: '`git worktree prune` drops the record of every worktree whose folder is missing right now, also a live agent\'s',
  force: '`git worktree remove --force` in a loop removes every worktree it lists, also a live agent\'s',
  docker: 'a container-engine `prune` or a removal of every listed object reaches objects of other agents and of the user, not only this task\'s',
  clean: '`git clean` with `-d`, `-x` or `-X` and no pathspec removes files across the whole checkout, also other tasks\' folders',
  rm: '`rm -r` on a bare variable, `~`, `.` or a glob deletes whatever it holds, an empty variable the parent',
  rmPs: '`Remove-Item -Recurse` on a bare variable or glob deletes whatever the variable holds',
};

/** `-f`, `--filter` and `--filter=...` narrow a listing. */
const FILTERED = /(?:^|\s)(?:-f|--filter)(?:[=\s]|$)/;

const lineAt = (text, index) => text.slice(0, index).split('\n').length;
const lineOf = (text, index) => {
  const a = text.lastIndexOf('\n', index - 1) + 1;
  const b = text.indexOf('\n', index);
  return text.slice(a, b < 0 ? text.length : b);
};
const unquote = (t) => t.replace(/["'`]/g, '');

/** Whether `flags` (words like `-fdx`, `--dry-run`) clean untracked directories or ignored files, and are not a dry run. */
function cleansWide(flags) {
  if (flags.some((w) => w === '--dry-run' || /^-[a-zA-Z]*n[a-zA-Z]*$/.test(w))) return false;
  return flags.some((w) => /^-[a-zA-Z]*[xXd][a-zA-Z]*$/.test(w));
}

/** `git clean` words (after `clean`): is it a wide clean with no real pathspec? */
function wideClean(words) {
  const flags = [];
  const paths = [];
  let rest = false;
  for (let k = 0; k < words.length; k++) {
    const w = unquote(words[k]);
    if (rest) paths.push(w);
    else if (w === '--') rest = true;
    else if (w === '--exclude' || /^-[a-zA-Z]*e$/.test(w)) {
      flags.push(w.replace(/e$/, ''));
      k++;
    } else if (w.startsWith('-')) flags.push(w);
    else paths.push(w);
  }
  return cleansWide(flags) && paths.every((p) => ['.', './', ':/', ''].includes(p));
}

/** Names the file assigns from `mktemp`: the script's own folders. */
const tempNames = (code) => {
  const names = new Set([...code.matchAll(/(?:^|[\s;&(])(?:(?:local|export|declare|readonly)\s+)?(\w+)=["']?(?:\$\(|`)\s*mktemp\b/g)].map((m) => m[1]));
  // `stage="$dir/setup-disc"`: a value with a literal path part below a variable is a folder the script names itself. A segment of
  // only dots (`"$ROOT/.."`, `$HOME/.`) climbs out of the variable's folder, so it does not count.
  for (const m of code.matchAll(/(?:^|[\s;&(])(?:(?:local|export|declare|readonly)\s+)?(\w+)=["']?([^\s"';&|)]*)/g)) {
    const value = m[2];
    if (/\$\{?\w+\}?\/[A-Za-z0-9._-]+/.test(value) && !value.split('/').some((seg) => /^\.+$/.test(seg))) names.add(m[1]);
  }
  return names;
};

/** Whether an `rm` target is bare: an unguarded variable, `~`, `.`, `*`, `/*` (not a mktemp variable of this file). */
function bareTarget(raw, temps) {
  const t = unquote(raw);
  if (/^(?:\.\/?|\.\.\/?|~(?:\/\*?)?|\*|\.\/\*|\/\*)$/.test(t)) return true;
  const v = /^\$(?:(\w+)|\{(\w+)([^}]*)\})(?:\/\*)?$/.exec(t);
  if (!v) return false;
  if (v[3] && /^:?\?/.test(v[3])) return false; // ${VAR:?}: fails when empty
  return !temps.has(v[1] ?? v[2]);
}

/** `text` with the full-line comments blanked (same length, newlines kept). */
function maskComments(text, file) {
  const slash = CODE.test(file);
  return text
    .split('\n')
    .map((l) => ((slash ? /^\s*\/\//.test(l) : /^\s*#/.test(l)) ? ' '.repeat(l.length) : l))
    .join('\n');
}

/** Whether shell text is inside a `for`/`while`/`until` loop at `index`: opened and not yet closed by `done`. */
function inShellLoop(code, index) {
  const before = code.slice(Math.max(0, index - 4000), index).replace(/"[^"\n]*"|'[^'\n]*'/g, '""');
  const opens = (before.match(/(?:^|[\s;&|({])(?:for|while|until)\s/g) ?? []).length;
  const closes = (before.match(/(?:^|[\s;&|)}])done\b/g) ?? []).length;
  return opens > closes;
}

/** The unscoped cleanups of one file: [{ line, what, fix, waived: 'reason' | 'no-reason' | undefined }]. */
export function unscopedCleanups(text, file = 'x.sh') {
  const code = maskComments(text, file);
  const isCode = CODE.test(file);
  const lines = text.split('\n');
  const hits = [];
  const add = (index, what, fix) => hits.push({ index, what, fix });
  // Text forms in Rust and JS count only on a line that runs a command; array forms always.
  const textOk = (index) => !isCode || RUNS.test(unstring(lineOf(code, index)));
  const near = (index) => code.slice(Math.max(0, index - 160), index);

  for (const m of code.matchAll(new RegExp(`\\bworktree(${Q}?\\s*,\\s*${Q}|\\s+)prune\\b`, 'g'))) {
    const array = m[1].includes(',');
    const tail = code.slice(m.index, m.index + 80).split('\n')[0];
    if ((array || textOk(m.index)) && !/(?:^|\s)(?:-n|--dry-run)\b/.test(tail)) add(m.index, WHAT.prune, PRUNE_FIX);
  }
  for (const m of code.matchAll(new RegExp(`\\bworktree(${Q}?\\s*,\\s*${Q}|\\s+)remove\\b`, 'g'))) {
    const array = m[1].includes(',');
    const tail = code.slice(m.index, m.index + 120).split('\n')[0];
    if (!(array || textOk(m.index)) || !/(?:^|[\s"'`,])(?:-f|--force)(?:[\s"'`,)\]]|$)/.test(tail)) continue;
    const before = lineOf(code, m.index).slice(0, m.index - (code.lastIndexOf('\n', m.index - 1) + 1));
    const underXargs = /\bxargs\b[^;&]*$/.test(before);
    const loop = isCode ? /\b(?:for|while|loop)\b|\.iter\(|\.forEach\(|\.map\(/.test(near(m.index).slice(-500)) : inShellLoop(code, m.index);
    if (underXargs || loop) add(m.index, WHAT.force, PRUNE_FIX);
  }
  for (const m of code.matchAll(new RegExp(`\\b${ENGINE}\\s+(?:-\\S+\\s+)*${KINDS}\\s+prune\\b`, 'g'))) if (textOk(m.index)) add(m.index, WHAT.docker, DOCKER_FIX);
  for (const m of code.matchAll(new RegExp(`\\b${KINDS}${Q}\\s*,\\s*${Q}prune\\b`, 'g'))) if (new RegExp(`\\b${ENGINE}\\b`).test(near(m.index))) add(m.index, WHAT.docker, DOCKER_FIX);
  const mass = new RegExp(
    `\\b${ENGINE}\\s+(?:-\\S+\\s+)*${KIND}${STOP}\\b[^\\n]*(?<l1>(?:\\$\\(|\`)\\s*${ENGINE}\\s+${KIND}${LIST}\\b[^\\n)\`]*)` +
      `|(?<l2>\\b${ENGINE}\\s+${KIND}${LIST}\\b[^\\n|]*)\\|\\s*xargs\\s+(?:-\\S+\\s+)*${ENGINE}\\s+${KIND}${STOP}\\b`,
    'g',
  );
  for (const m of code.matchAll(mass)) if (textOk(m.index) && !FILTERED.test(m.groups.l1 ?? m.groups.l2)) add(m.index, WHAT.docker, DOCKER_FIX);

  // git clean, as a command line or an argument array.
  for (const m of code.matchAll(/\bgit\b[^\n;|&]*?\bclean\b[ \t]+([^\n;|&)]*)/g)) {
    if (textOk(m.index) && wideClean(m[1].split(/\s+/).filter(Boolean))) add(m.index, WHAT.clean, CLEAN_FIX);
  }
  for (const m of code.matchAll(new RegExp(`${Q}clean${Q}\\s*,\\s*((?:${Q}[^"'\`\\n]*${Q}\\s*,?\\s*)+)\\]`, 'g'))) {
    if (wideClean([...m[1].matchAll(/["'`]([^"'`\n]*)["'`]/g)].map((x) => x[1]))) add(m.index, WHAT.clean, CLEAN_FIX);
  }

  if (!isCode) {
    const temps = tempNames(code);
    for (const m of code.matchAll(/\brm[ \t]+([^\n;&|)#]*)/g)) {
      const words = m[1].split(/\s+/).filter(Boolean);
      const flags = [];
      const targets = [];
      let rest = false;
      for (const w of words) {
        if (!rest && w === '--') rest = true;
        else if (!rest && w.startsWith('-') && w.length > 1) flags.push(w);
        else targets.push(w);
      }
      const recursive = flags.some((w) => w === '--recursive' || /^-[a-zA-Z]*[rR][a-zA-Z]*$/.test(w));
      if (recursive && targets.some((t) => bareTarget(t, temps))) add(m.index, WHAT.rm, RM_FIX);
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
      if (target !== undefined && psBare.test(target)) add(m.index, WHAT.rmPs, RM_FIX);
    }
  }
  return hits
    .sort((a, b) => a.index - b.index)
    .map(({ index, what, fix }) => {
      const line = lineAt(code, index);
      const around = [lines[line - 1] ?? '', lines[line - 2] ?? ''].join('\n');
      const waived = WAIVER.test(around) ? 'reason' : BARE_WAIVER.test(around) ? 'no-reason' : undefined;
      return { line, what, fix, waived };
    });
}

/** Whether a tracked file is read by C115: its extension, or an extensionless script with a shell shebang. */
export const isCleanupSource = (file, head = '') => !SKIP_PATH.test(file) && (SOURCE.test(file) || (!/\.[^/]+$/.test(file) && /^#!.*\b(?:ba|z)?sh\b/.test(head)));

/** Every site of a file tree: { sites (to judge), waived (let through by a comment), noReason (waiver without a reason) }. */
export function cleanupSites(files, read) {
  const out = { sites: [], waived: [], noReason: [] };
  for (const f of files) {
    if (SKIP_PATH.test(f)) continue;
    const text = read(f);
    if (!text || !isCleanupSource(f, text.slice(0, 80))) continue;
    if (!/prune|\bclean\b|\brm\b|Remove-Item|docker|podman|worktree/.test(text)) continue;
    const lines = text.split('\n');
    for (const k of unscopedCleanups(text, f)) {
      const at = { file: f, line: k.line, what: k.what, fix: k.fix, text: lines[k.line - 1] ?? '', hash: lineHash(lines[k.line - 1] ?? '') };
      if (k.waived === 'reason') out.waived.push(at);
      else if (k.waived === 'no-reason') out.noReason.push(at);
      else out.sites.push(at);
    }
  }
  return out;
}

export default {
  id: 'C115',
  scope: 'both',
  mode: 'gate',
  title: 'A cleanup touches only its own items',
  rule: 2,
  run(ctx) {
    const out = [];
    const { sites, waived, noReason } = cleanupSites(ctx.files(), ctx.read);
    for (const s of noReason) out.push(error('C115', 2, s.file, s.line, 'the scoped-cleanup waiver gives no reason', 'Write `// conformance: scoped-cleanup — <why it only touches its own items>`.'));
    let baseline = { sites: [] };
    const raw = ctx.read(BASELINE_PATH);
    if (raw) baseline = JSON.parse(raw);
    const { fresh, stale, moved } = againstBaseline(sites, baseline.sites, sourceGone(ctx));
    for (const m of moved) out.push(movedWarning('C115', 2, BASELINE_PATH, m));
    for (const s of fresh) out.push(error('C115', 2, s.file, s.line, `${s.what} (ER-2, ER-14): ${s.text.trim().slice(0, 100)}`, s.fix));
    for (const e of stale) out.push(warn('C115', 2, e.file, e.line ?? 1, `${BASELINE_PATH} lists a site that is gone (hash ${e.hash}); the baseline may only shrink`, 'Remove the entry (`node conformance/bin/paddock-cleanup-baseline.mjs --shrink`).'));
    if (waived.length) {
      const list = waived.slice(0, 8).map((s) => `${s.file}:${s.line}`).join(', ');
      out.push(warn('C115', 2, BASELINE_PATH, 1, `${waived.length} site${waived.length === 1 ? '' : 's'} waived by a scoped-cleanup comment: ${list}${waived.length > 8 ? ', ...' : ''}`, 'Check that each reason still holds when the site changes.'));
    }
    if (baseline.sites.length) {
      const g = baselineGrowth(ctx, baseline.sites, BASELINE_PATH);
      if (g.reason) out.push(warn('C115', 2, BASELINE_PATH, 1, `the baseline's shrink-only rule is not enforced here: ${g.reason}`, 'Fetch the target branch (`git fetch origin main`, full history), or set PADDOCK_BASE_REF to the ref to compare with.'));
      else for (const m of g.moved) out.push(growthMovedWarning('C115', 2, BASELINE_PATH, m));
      if (!g.reason && g.added.length) out.push(error('C115', 2, BASELINE_PATH, 1, `the baseline grew: ${g.added.length} entr${g.added.length === 1 ? 'y is' : 'ies are'} not in the baseline at the merge base ${g.base.slice(0, 9)}; it may only shrink`, 'Remove the entries and scope the cleanups (an owned prefix, `"${VAR:?}"`, or `// conformance: scoped-cleanup — <why>`). Growing the baseline needs the user\'s approval.'));
    }
    return out;
  },
};
