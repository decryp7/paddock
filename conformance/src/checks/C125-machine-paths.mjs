// C125: no hard-coded machine disk paths (rule 40; ER-21 class "stale machine paths after a disk move"). A `/Volumes/<name>` literal
// names one disk of one machine: when the disk is renamed or replaced the literal goes stale and whatever reads it quietly stops working.
// It happened to the tart link, the app plists, and the worktree and docker guards, which defaulted to a disk that no longer existed
// and so switched the ER-2 check off (fix/guard-paths-derived, a5907d18c, 2026-11-14). Derive the path instead (git, `paddock.worktreeRoot`,
// an environment variable, a setting) and fail closed when it cannot be derived.
//
// Flagged: every `/Volumes/<name>` literal (the name starts with a letter or digit, so `/Volumes/<disk>` and `/Volumes/...` are
// placeholders, not paths; a letter, digit, `_`, `.` or `~` right before it, as in `/System/Volumes/Data`, makes it part of
// another path) in every text file of the tree. One finding per literal.
// Not read:
// - documents that record history, listed by path in the `history` array of `conformance/c125-allow.json`, each with a reason. They say
//   what was true when they were written (a lesson, a benchmark, an ADR, a research note, a dated plan); rewriting their paths would
//   falsify the record, and no tool reads a path from them. Skipped rather than allowed site by site: their count is not a ratchet,
//   they only ever grow. A history entry that names no file is stale (a warning).
// - the check's own files (its sources, tests and allow list name the pattern).
// Allowed: one entry per site in the `allow` array of the same file: a `path`, a `site` (a piece of the flagged literal), a `reason`
// and a `count` (default 1). An entry that matches fewer literals than its count is stale and one that matches more is ambiguous:
// both are warnings. The allow list has the shape of `conformance/src/util/allow-list.mjs` (fix/allow-list-helper, the train 11
// integrator's branch, not on the remote when this was written); `AllowList` below moves onto that helper once it lands.
// Report-only (warnings) until its first green run on the real tree is recorded here (ER-41). Recorded: not yet.
// Rust host: tools/paddock-dev/src/check/c125.rs (same findings, line for line).
import { unwaivable, warn } from '../finding.mjs';

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
const LITERAL = /\/Volumes\/[A-Za-z0-9][A-Za-z0-9_.+@-]*(?:\/[A-Za-z0-9_.+@-]*)*/g;
const BEFORE = /[A-Za-z0-9_.~]/;
const FIX = 'Derive the folder (git, `paddock.worktreeRoot`, an environment variable or a setting) and fail closed when it cannot; invented test data and documents that must name a disk get an entry in conformance/c125-allow.json with a reason.';

/** [{ line, literal }] of every machine disk path in `text`. */
export function machinePaths(text) {
  const out = [];
  if (!text.includes('/Volumes/')) return out;
  let line = 1;
  let from = 0;
  for (const m of text.matchAll(LITERAL)) {
    if (m.index > 0 && BEFORE.test(text[m.index - 1])) continue;
    for (let i = from; i < m.index; i++) if (text.charCodeAt(i) === 10) line++;
    from = m.index;
    out.push({ line, literal: m[0] });
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

/** The allow file's two lists: `history` [{ path, reason, re }] and `allow` [{ path, site, reason, count }], or throws why it cannot be read. */
export function parseAllow(text) {
  let v;
  try {
    v = JSON.parse(text);
  } catch (e) {
    throw new Error(`${ALLOW} is not JSON: ${e.message}`);
  }
  if (!Array.isArray(v?.allow) || !Array.isArray(v?.history)) throw new Error(`${ALLOW} needs a \`history\` and an \`allow\` array`);
  const reasonOf = (e) => (typeof e?.reason === 'string' ? e.reason.trim() : '');
  const history = v.history.map((e) => {
    const path = typeof e?.path === 'string' ? e.path : '';
    if (!path || [...reasonOf(e)].length < 8) throw new Error(`${ALLOW}: every history entry needs a path and a reason of at least 8 characters`);
    return { path, reason: reasonOf(e), re: globRegExp(path) };
  });
  const allow = v.allow.map((e) => {
    const path = typeof e?.path === 'string' ? e.path : '';
    const site = typeof e?.site === 'string' ? e.site.trim() : '';
    const count = e?.count === undefined ? 1 : e.count;
    if (!path || !site || [...reasonOf(e)].length < 8) {
      throw new Error(`${ALLOW}: every entry needs a path, a site (a piece of the flagged path) and a reason of at least 8 characters`);
    }
    if (!Number.isInteger(count) || count < 1) throw new Error(`${ALLOW}: \`count\` (${path}: ${site}) is a whole number of at least 1`);
    return { path, site, reason: reasonOf(e), count };
  });
  return { history, allow };
}

/** The allow list of one run (the shape of fix/allow-list-helper's `AllowList`, which replaces it when it lands). */
export class AllowList {
  constructor(file, entries) {
    this.file = file;
    this.entries = entries;
    this.hits = entries.map(() => 0);
  }

  /** Whether an entry names the site at `path` whose text is `text`; every entry that does counts the site. */
  allows(path, text) {
    let named = false;
    this.entries.forEach((a, i) => {
      if (a.path === path && text.includes(a.site)) {
        this.hits[i]++;
        named = true;
      }
    });
    return named;
  }

  /** The warnings for entries that matched another number of sites than their count; `looked(path)`: the run read it or it does not exist. */
  warnings(id, rule, looked) {
    const out = [];
    this.entries.forEach((a, i) => {
      const hits = this.hits[i];
      if (hits > a.count) {
        out.push(warn(id, rule, this.file, 1, `ambiguous allow entry (${a.path}: ${a.site}) matches ${hits} sites`, a.count > 1 ? `Lengthen its \`site\`, or raise its \`count\` to ${hits} if every one is meant.` : 'Lengthen its `site` so it names one line.'));
      } else if (hits < a.count && looked(a.path)) {
        const what = hits === 0 ? 'matches no site' : `matches only ${hits} of its ${a.count} sites`;
        out.push(warn(id, rule, this.file, 1, `stale allow entry (${a.path}: ${a.site}) ${what}`, hits === 0 ? `Remove it from ${this.file}.` : `Lower its \`count\` to ${hits}.`));
      }
    });
    return out;
  }
}

export default {
  id: 'C125',
  scope: 'root',
  mode: 'report-only',
  title: 'No hard-coded machine disk paths',
  rule: 40,
  run(ctx) {
    let lists;
    try {
      const raw = ctx.read(ALLOW);
      if (raw === null || raw === undefined) throw new Error(`${ALLOW} cannot be read`);
      lists = parseAllow(raw);
    } catch (e) {
      return [unwaivable(warn('C125', 40, ALLOW, 1, `the check cannot look: ${e.message}`, 'Restore conformance/c125-allow.json: {"history": [{"path": "...", "reason": "..."}], "allow": [{"path": "...", "site": "...", "reason": "...", "count": 1}]}.'), 'check-broken')];
    }
    const allow = new AllowList(ALLOW, lists.allow);
    const historyHits = lists.history.map(() => 0);
    const out = [];
    const scanned = new Set();
    for (const f of ctx.files()) {
      if (IMPLEMENTATION.has(f) || CHECKOUT.has(f) || BINARY.test(f)) continue;
      const h = lists.history.findIndex((e) => e.re.test(f));
      if (h >= 0) {
        historyHits[h]++;
        continue;
      }
      const text = ctx.read(f);
      if (text === null || text === undefined || text.includes('\0')) continue;
      scanned.add(f);
      for (const { line, literal } of machinePaths(text)) {
        if (allow.allows(f, literal)) continue;
        out.push(warn('C125', 40, f, line, `hard-codes the machine disk path \`${literal}\`: it goes stale when the disk is renamed or replaced`, FIX));
      }
    }
    lists.history.forEach((e, i) => {
      if (historyHits[i] === 0) out.push(warn('C125', 40, ALLOW, 1, `stale history entry (${e.path}) names no file`, `Remove it from ${ALLOW}.`));
    });
    const looked = (path) => scanned.has(path) || ctx.read(path) === null || ctx.read(path) === undefined;
    return [...out, ...allow.warnings('C125', 40, looked)];
  },
};
