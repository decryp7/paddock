// C129: no new shell (rule 55's cousin; the shell-to-Rust plan, docs/plans/shell-to-rust.md §6 and WP-0; ER-21 class "a shell script
// behaves differently on macOS, Linux and Git Bash": run 8138, the prune and du/stat fixes of train 12). Shell and Python scripts are
// being ported to Rust; this check keeps new ones out while the port runs.
// Flagged, one finding per file at line 1, in every file of the tree:
// - a file named `*.sh`, `*.bash`, `*.zsh`, `*.ps1`, `*.psm1`, `*.py`, `*.cmd` or `*.bat`, or an extensionless file whose first line is a
//   `#!` naming `sh`, `bash`, `zsh`, `python` or `pwsh` (also through `env`),
// unless it is
// - an exception: a path or pattern in `conformance/shell-exceptions.json` with its plan id (E1 to E7), a reason and an owner. A
//   pattern is a path where `**/` is any number of folders, `*` one name and a trailing `/` the folder and all below it; or
// - in the baseline `conformance/shell-baseline.json`, which lists today's files with their line counts, one per line, sorted by path.
//   A file with more lines than its entry has grown (a finding); fewer is fine. The baseline may only shrink: an entry whose file is gone
//   is stale (a warning), and an entry the merge base's baseline lacks, or one with more lines than there, is a finding. Regenerate it
//   with `node conformance/bin/paddock-shell-baseline.mjs --shrink` (removes stale entries and lowers counts, never adds or raises;
//   `--init` writes it from scratch: first run, or a reset with the user's approval).
// Also reported, never failing the run: a `.sh` or `.ps1` path that `.gitea` workflows and actions, `paddock.ci.json` files and
// `lefthook.yml` name but the tree lacks (a deleted script keeps a caller), and one summary line with the count of inline `run:` lines
// in `.gitea` (the number D7 of the plan is decided on).
// Read: only files the tree lists; shebangs only of extensionless files (an extension says what a file is). A file that cannot be read is
// not measured (an error at that file); an unreadable or malformed baseline or exception list makes the check broken (one finding).
// ER-41: report-only (warnings) until its first green run on the real tree, recorded by run id in docs/gates.toml; then the findings
// become errors and the mode gate. Recorded: not yet.
// Rust host: tools/paddock-dev/src/check/c129.rs (same findings, line for line).
import { error, unwaivable, warn } from '../finding.mjs';
import { baselineGrowth } from '../util/baseline.mjs';

export const BASELINE = 'conformance/shell-baseline.json';
export const EXCEPTIONS = 'conformance/shell-exceptions.json';
const REGENERATE = 'node conformance/bin/paddock-shell-baseline.mjs --shrink';
const KINDS = /\.(?:sh|bash|zsh|ps1|psm1|py|cmd|bat)$/i;
const SHEBANG = /^#![ \t]*(?:\S*\/)?(?:env[ \t]+(?:-\S+[ \t]+)*)?(?:sh|bash|zsh|python[0-9.]*|pwsh)(?:[ \t]|$)/;
const EXCEPTION_IDS = ['E1', 'E2', 'E3', 'E4', 'E5', 'E6', 'E7'];
const SCRIPT_WORD = /^(?:\.\/)?(?:[A-Za-z0-9_.@-]+\/)+[A-Za-z0-9_.@-]+\.(?:sh|ps1)$/;
const NOT_A_WORD = /[^A-Za-z0-9_.@/$*{}-]+/;
const RUN_LINE = /^([ \t]*)(-[ \t]+)?run:[ \t]*(.*)$/;
const RUN_BLOCK = /^[|>][+-]?[0-9]*[ \t]*(?:#.*)?$/;
const FIX_NEW = `Port it to Rust (\`./dev\`, an xtask or a check), or name it in ${EXCEPTIONS} with the plan's exception id and a reason (docs/plans/shell-to-rust.md §2).`;

/** A path pattern as a regular expression: `**` is any number of folders, `*` one name, a trailing `/` the folder and all below it. */
export function globRegExp(pattern) {
  let src = '';
  for (let i = 0; i < pattern.length; i++) {
    const c = pattern[i];
    if (c === '*' && pattern[i + 1] === '*' && pattern[i + 2] === '/') {
      src += '(?:[^/]+/)*';
      i += 2;
    } else if (c === '*') src += '[^/]*';
    else src += c.replace(/[.+?^${}()|[\]\\]/g, '\\$&');
  }
  return new RegExp(`^${src}${pattern.endsWith('/') ? '.*' : ''}$`);
}

const parseJson = (text, file) => {
  try {
    return JSON.parse(text);
  } catch (e) {
    throw new Error(`${file} is not JSON: ${e.message}`);
  }
};

/** The baseline's entries `[{ path, lines }]`, or throws why it cannot be read. */
export function parseBaseline(text) {
  const v = parseJson(text, BASELINE);
  if (!Array.isArray(v?.files)) throw new Error(`${BASELINE} has no \`files\` array`);
  return v.files.map((e) => {
    const path = typeof e?.path === 'string' ? e.path : '';
    if (!path || !Number.isInteger(e?.lines) || e.lines < 0) throw new Error(`${BASELINE}: every entry needs a path and a whole number of lines`);
    return { path, lines: e.lines };
  });
}

/** The exceptions `[{ path, id, reason, owner, re }]`, or throws why the list cannot be read. */
export function parseExceptions(text) {
  const v = parseJson(text, EXCEPTIONS);
  if (!Array.isArray(v?.exceptions)) throw new Error(`${EXCEPTIONS} has no \`exceptions\` array`);
  return v.exceptions.map((e) => {
    const path = typeof e?.path === 'string' ? e.path : '';
    const reason = typeof e?.reason === 'string' ? e.reason.trim() : '';
    const owner = typeof e?.owner === 'string' ? e.owner.trim() : '';
    if (!path || !EXCEPTION_IDS.includes(e?.id) || [...reason].length < 8 || !owner) {
      throw new Error(`${EXCEPTIONS}: every exception needs a path, an id from E1 to E7, a reason of at least 8 characters and an owner`);
    }
    return { path, id: e.id, reason, owner, re: globRegExp(path) };
  });
}

/** The number of lines of `text` (a last line without a newline counts). */
export const lineCount = (text) => (text.length === 0 ? 0 : text.split('\n').length - (text.endsWith('\n') ? 1 : 0));

const baseName = (f) => f.slice(f.lastIndexOf('/') + 1);
const dirName = (f) => (f.includes('/') ? f.slice(0, f.lastIndexOf('/')) : '');

/** Whether the file may be shell: by extension, or an extensionless file with a shell shebang (`text` is read for those only). */
export function isShell(f, readText) {
  if (KINDS.test(f)) return true;
  if (baseName(f).lastIndexOf('.') > 0) return false;
  const text = readText();
  if (text === null || text === undefined || text.includes('\0')) return false;
  return SHEBANG.test(text.split('\n', 1)[0].replace(/\r$/, ''));
}

/**
 * The shell files of a tree not covered by an exception: `{ shell: [{ file, lines }], excepted: Map(exception index -> hits), unread: [file] }`
 * (`files` sorted, `read(path)` the text or null).
 */
export function shellFiles(files, read, exceptions) {
  const shell = [];
  const unread = [];
  const hits = new Map();
  for (const f of files) {
    let text;
    let failed = false;
    const readText = () => {
      text = read(f);
      failed = text === null || text === undefined;
      return text;
    };
    if (!isShell(f, readText)) {
      if (failed) unread.push(f);
      continue;
    }
    const at = exceptions.findIndex((e) => e.re.test(f));
    if (at >= 0) {
      hits.set(at, (hits.get(at) ?? 0) + 1);
      continue;
    }
    if (text === undefined) readText();
    if (failed) unread.push(f);
    else shell.push({ file: f, lines: lineCount(text) });
  }
  return { shell, hits, unread };
}

/** The `.sh` and `.ps1` words of a line that look like a repository path (no variable, no wildcard). */
const scriptWords = (line) => (line.trimStart().startsWith('#') ? [] : line.split(NOT_A_WORD).filter((w) => SCRIPT_WORD.test(w)).map((w) => w.replace(/^\.\//, '')));

/** Which files name callers of scripts: `.gitea` workflows and actions, `paddock.ci.json` files and `lefthook.yml`. */
const isCaller = (f) => (f.startsWith('.gitea/') && /\.ya?ml$/.test(f)) || baseName(f) === 'paddock.ci.json' || f === 'lefthook.yml';

/** Inline `run:` lines of one workflow or action file: `{ blocks, lines }` (non-blank, non-comment lines of each `run:` value). */
export function inlineRun(text) {
  const rows = text.split('\n');
  let blocks = 0;
  let lines = 0;
  for (let i = 0; i < rows.length; i++) {
    const m = RUN_LINE.exec(rows[i].replace(/\r$/, ''));
    if (!m || rows[i].trimStart().startsWith('#')) continue;
    const rest = m[3].trim();
    if (RUN_BLOCK.test(rest)) {
      const key = m[1].length + (m[2] ? m[2].length : 0);
      let n = 0;
      let j = i + 1;
      for (; j < rows.length; j++) {
        const row = rows[j].replace(/\r$/, '');
        if (row.trim() === '') continue;
        if (row.length - row.trimStart().length <= key) break;
        if (!row.trimStart().startsWith('#')) n++;
      }
      blocks++;
      lines += n;
      i = j - 1;
    } else if (rest !== '' && !rest.startsWith('#')) {
      blocks++;
      lines++;
    }
  }
  return { blocks, lines };
}

const plural = (n, one, many) => (n === 1 ? one : many);
const broken = (file, why, fix) => [unwaivable(warn('C129', 55, file, 1, `the check cannot look: ${why}`, fix), 'check-broken')];

export default {
  id: 'C129',
  scope: 'root',
  mode: 'report-only',
  title: 'No new shell',
  rule: 55,
  run(ctx) {
    let baseline;
    let exceptions;
    for (const [file, parse, restore] of [
      [BASELINE, parseBaseline, `Regenerate it: \`node conformance/bin/paddock-shell-baseline.mjs --init\` (the first run only), or restore it from git.`],
      [EXCEPTIONS, parseExceptions, `Restore ${EXCEPTIONS}: {"exceptions": [{"path": "...", "id": "E1", "reason": "...", "owner": "..."}]}.`],
    ]) {
      try {
        const raw = ctx.read(file);
        if (raw === null || raw === undefined) throw new Error(`${file} cannot be read`);
        if (file === BASELINE) baseline = parse(raw);
        else exceptions = parse(raw);
      } catch (e) {
        return broken(file, e.message, restore);
      }
    }
    const files = ctx.files();
    const out = [];
    const { shell, hits, unread } = shellFiles(files, ctx.read, exceptions);
    for (const f of unread) out.push(unwaivable(error('C129', 55, f, 1, `not measured: ${f} could not be read, so shell in it cannot be seen`, 'Make the file readable (permissions) and run the check again.'), 'not-measured'));
    const known = new Map(baseline.map((e) => [e.path, e.lines]));
    for (const { file, lines } of shell) {
      if (!known.has(file)) out.push(warn('C129', 55, file, 1, `new shell file (${lines} lines): shell is being ported to Rust`, FIX_NEW));
      else if (lines > known.get(file)) out.push(warn('C129', 55, file, 1, `shell file grew from ${known.get(file)} to ${lines} lines: the baseline may only shrink`, `Port the change to Rust instead of growing the script. ${FIX_NEW}`));
    }
    const current = new Set(shell.map((s) => s.file));
    for (const e of baseline) {
      // a file the run could not read is not judged: the error at that file says so
      if (!current.has(e.path) && !unread.includes(e.path)) out.push(warn('C129', 55, BASELINE, 1, `stale baseline entry (${e.path}): the file is gone or is no longer shell`, `Remove it: \`${REGENERATE}\`.`));
    }
    exceptions.forEach((e, i) => {
      if (!hits.has(i) && !files.some((f) => e.re.test(f))) out.push(warn('C129', 55, EXCEPTIONS, 1, `stale exception (${e.path}, ${e.id}) names no file`, `Remove it from ${EXCEPTIONS}.`));
    });
    if (baseline.length > 0) {
      const g = baselineGrowth({ root: ctx.root, path: BASELINE, current: baseline, keyOf: (e) => `${e.path}\t${e.lines}`, looseKeyOf: (e) => e.path, entriesOf: (v) => v.files.map((e) => ({ path: e.path, lines: e.lines })) });
      if (g.reason) out.push(warn('C129', 55, BASELINE, 1, `the baseline's shrink-only rule is not enforced here: ${g.reason}`, 'Fetch the target branch (`git fetch origin main`, full history), or set PADDOCK_BASE_REF to the ref to compare with.'));
      else {
        for (const m of g.moved) {
          if (m.item.lines > m.from.lines) out.push(warn('C129', 55, BASELINE, 1, `the baseline raised ${m.item.path} from ${m.from.lines} to ${m.item.lines} lines; it may only shrink`, 'Lower the entry again and port the change to Rust instead.'));
        }
        if (g.added.length > 0) {
          out.push(warn('C129', 55, BASELINE, 1, `the baseline grew: ${g.added.length} ${plural(g.added.length, 'entry is', 'entries are')} not in the baseline at the merge base ${g.base.slice(0, 9)}; it may only shrink`, 'Remove the entries and port the scripts. Growing the baseline needs the user\'s approval.'));
        }
      }
    }
    const tracked = new Set(files);
    const apps = [...new Set(files.filter((f) => f.startsWith('apps/') && f.split('/').length > 2).map((f) => f.split('/')[1]))];
    const tops = new Set(files.filter((f) => f.includes('/')).map((f) => f.split('/')[0]));
    for (const f of files) {
      if (!isCaller(f)) continue;
      const text = ctx.read(f);
      if (text === null || text === undefined) continue;
      const here = dirName(f);
      text.split('\n').forEach((row, i) => {
        for (const w of scriptWords(row)) {
          if (!tops.has(w.split('/')[0])) continue;
          // a path is read from the repository root, from the caller's folder, or from an app's folder (`working-directory: apps/<app>`)
          if (tracked.has(w) || (here !== '' && tracked.has(`${here}/${w}`)) || apps.some((a) => tracked.has(`apps/${a}/${w}`))) continue;
          out.push(warn('C129', 55, f, i + 1, `names the script \`${w}\`, which the tree does not have`, 'Switch the caller to the command that replaced the script, or restore the script.'));
        }
      });
    }
    let blocks = 0;
    let lines = 0;
    for (const f of files) {
      if (!f.startsWith('.gitea/') || !/\.ya?ml$/.test(f)) continue;
      const text = ctx.read(f);
      if (text === null || text === undefined) continue;
      const r = inlineRun(text);
      blocks += r.blocks;
      lines += r.lines;
    }
    if (blocks > 0) out.push(warn('C129', 55, '.gitea', 1, `inline shell in .gitea: ${lines} lines in ${blocks} \`run:\` ${plural(blocks, 'block', 'blocks')} (reported, not a finding of a file)`, 'Decision D7 of docs/plans/shell-to-rust.md is taken on this number.'));
    return out;
  },
};
