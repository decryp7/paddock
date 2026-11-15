// C125: no hard-coded machine disk paths (rule 40; ER-21 class "stale machine paths after a disk move"). A `/Volumes/<name>` literal
// names one disk of one machine: when the disk is renamed or replaced the literal goes stale and whatever reads it quietly stops working.
// It happened to the tart link, the app plists, and the worktree and docker guards, which defaulted to a disk that no longer existed
// and so switched the ER-2 check off (fix/guard-paths-derived, a5907d18c, 2026-11-14). Derive the path instead (git, `paddock.worktreeRoot`,
// an environment variable, a setting) and fail closed when it cannot be derived.
//
// Flagged: every `/Volumes/<name>` literal, also with its slashes escaped (`\/Volumes\/<name>`, a regular expression), reported
// unescaped (the name starts with a letter or digit, so `/Volumes/<disk>` and `/Volumes/...` are
// placeholders, not paths; a letter, digit, `_`, `.` or `~` right before it, as in `/System/Volumes/Data`, makes it part of
// another path) in every text file of the tree. One finding per literal.
// Not read:
// - Markdown documents that record history, listed by path in the `history` array of `conformance/c125-allow.json`, each with a reason
//   (lessons, benchmarks, ADRs, audits, research notes). They say what was true when they were written; rewriting their paths would
//   falsify the record, and no tool reads a path from them. Only `.md` files are covered (a script or raw result beside them is read),
//   and their literals are counted, not reported: an entry that skipped no `/Volumes/` literal is stale (a warning). Dated plans are
//   not history: they are followed, so they are read.
// - the checkout's own `.git` file and owner note, and the check's own files (its sources, tests and allow list name the pattern).
// Allowed: per-site entries in the `allow` array of the same file, read by the shared allow list (conformance/src/util/allow-list.mjs):
// a `path`, a `site` and a `reason`, with a `count` (default 1). A site names a path by whole components: `/Volumes/Data` allows
// `/Volumes/Data` and `/Volumes/Data/x`, never `/Volumes/Database`. A literal beyond the count of the entries that name it is reported
// at its own line; an entry that matches fewer literals than its count is stale. A file that cannot be read is not measured (an
// error finding at that file), and an allow file that cannot be read makes the check broken (one finding, nothing else).
// Report-only (warnings) until its first green run on the real tree is recorded here (ER-41). Recorded: not yet.
// Rust host: tools/paddock-dev/src/check/c125.rs (same findings, line for line).
import { warn } from '../finding.mjs';
import { parseAllowList, readAllowList } from '../util/allow-list.mjs';

export const ALLOW = 'conformance/c125-allow.json';
/** The check's own files: they name the pattern. */
export const IMPLEMENTATION = new Set([
  'conformance/src/checks/C125-machine-paths.mjs',
  'conformance/test/c125-machine-paths.test.mjs',
  'tools/paddock-dev/src/check/c125.rs',
  'tools/paddock-dev/tests/it/check_c125.rs',
  ALLOW,
]);
/** The checkout's own files, never tracked: a worktree's `.git` file (its `gitdir:` line) and the worktree tool's owner note. */
export const CHECKOUT = new Set(['.git', '.paddock-owner.toml']);
/** Files that are never text: not read. */
const BINARY = /\.(?:png|jpe?g|gif|ico|icns|webp|bmp|tiff?|pdf|zip|gz|tgz|xz|zst|7z|whl|wasm|dmg|pkg|msi|exe|dll|so|dylib|a|lib|rlib|o|obj|pdb|ttf|otf|woff2?|mp3|mp4|mov|wav|webm|parquet|duckdb|sqlite|db|evtx|dmp|bin|onnx|safetensors)$/i;
// A `/` may be escaped (`\/`, a regular expression literal): the finding names the path unescaped.
const LITERAL = /\\?\/Volumes\\?\/[A-Za-z0-9][A-Za-z0-9_.+@-]*(?:\\?\/[A-Za-z0-9_.+@-]*)*/g;
const BEFORE = /[A-Za-z0-9_.~]/;
const BROKEN_FIX = 'Restore conformance/c125-allow.json: {"history": [{"path": "...", "reason": "..."}], "allow": [{"path": "...", "site": "...", "reason": "...", "count": 1}]}.';
const FIX = 'Derive the folder (git, `paddock.worktreeRoot`, an environment variable or a setting) and fail closed when it cannot; invented test data and documents that must name a disk get an entry in conformance/c125-allow.json with a reason.';

/** [{ line, literal }] of every machine disk path in `text`. */
export function machinePaths(text) {
  const out = [];
  if (!text.includes('Volumes')) return out;
  let line = 1;
  let from = 0;
  for (const m of text.matchAll(LITERAL)) {
    if (m.index > 0 && BEFORE.test(text[m.index - 1])) continue;
    for (let i = from; i < m.index; i++) if (text.charCodeAt(i) === 10) line++;
    from = m.index;
    out.push({ line, literal: m[0].replaceAll('\\/', '/') });
  }
  return out;
}

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

/** Whether the allow `site` names `literal` by whole path components. */
export const namesPath = (site, literal) => literal === site || literal.startsWith(site.endsWith('/') ? site : `${site}/`);

/** The allow file's two lists: `history` [{ path, reason, re }] and `allow` (the shared allow list's entries), or throws why it cannot be read. */
export function parseAllow(text) {
  const allow = parseAllowList(text, ALLOW);
  const v = JSON.parse(text);
  if (!Array.isArray(v?.history)) throw new Error(`${ALLOW} has no \`history\` array`);
  const history = v.history.map((e) => {
    const path = typeof e?.path === 'string' ? e.path : '';
    const reason = typeof e?.reason === 'string' ? e.reason.trim() : '';
    if (!path || [...reason].length < 8) throw new Error(`${ALLOW}: every history entry needs a path and a reason of at least 8 characters`);
    return { path, reason, re: globRegExp(path) };
  });
  return { history, allow };
}

const SPEC = {
  id: 'C125',
  rule: 40,
  file: ALLOW,
  unseen: 'a disk path',
  restore: BROKEN_FIX,
  parse: (text) => {
    const { history, allow } = parseAllow(text);
    return { entries: allow, data: history };
  },
};

export default {
  id: 'C125',
  scope: 'root',
  mode: 'report-only',
  allowList: ALLOW,
  title: 'No hard-coded machine disk paths',
  rule: 40,
  run(ctx) {
    const { list: allow, data: history, broken } = readAllowList(ctx, SPEC);
    if (broken) return broken;
    const historyHits = history.map(() => 0);
    const out = [];
    for (const f of ctx.files()) {
      if (IMPLEMENTATION.has(f) || CHECKOUT.has(f) || BINARY.test(f)) continue;
      const h = f.endsWith('.md') ? history.findIndex((e) => e.re.test(f)) : -1;
      const { text, finding } = allow.read(f);
      if (finding) {
        out.push(finding);
        continue;
      }
      if (text.includes('\0')) continue;
      for (const { line, literal } of machinePaths(text)) {
        if (h >= 0) {
          historyHits[h]++;
          continue;
        }
        const said = allow.admit(f, (site) => namesPath(site, literal));
        if (said === 'allowed') continue;
        out.push(
          said === 'beyond'
            ? warn('C125', 40, f, line, `hard-codes the machine disk path \`${literal}\` beyond the count of its allow entry`, `Derive it, or raise the entry's \`count\` in ${ALLOW} if it is invented data too.`)
            : warn('C125', 40, f, line, `hard-codes the machine disk path \`${literal}\`: it goes stale when the disk is renamed or replaced`, FIX),
        );
      }
    }
    history.forEach((e, i) => {
      if (historyHits[i] === 0) out.push(warn('C125', 40, ALLOW, 1, `stale history entry (${e.path}) skipped no disk path`, `Remove it from ${ALLOW}.`));
    });
    return [...out, ...allow.warnings()];
  },
};
