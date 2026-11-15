// One allow list per check, one site per entry (ER-21; PHPStan's `reportUnmatchedIgnoredErrors` and the `count` of its baseline).
// An entry names a file (`path`), a piece of the flagged line (`site`), a `reason` of at least 8 characters and, when the same line
// appears more than once in the file, how many sites it covers (`count`, default 1). A whole file or folder is never allowed.
// An entry allows at most `count` sites: a site beyond it is reported at its own file and line, as an unlisted one is. After a run, an
// entry that matched fewer sites than its count is stale (remove it, or lower its count), a warning, so the list only ever names the
// sites that still exist.
// The reader also owns the error paths, so every check that uses it has the same ones on both hosts (ER-21 class "the two hosts
// diverge on an error path": C125 in 865498235, then C121 and C124): an allow file that is missing, not JSON, without its list or with
// a malformed entry is one unwaivable check-broken finding and nothing else (`readAllowList`), and a file the check cannot read is a
// not-measured error at that file (`AllowList.read`).
// Used by C121, C124 and C125, each naming its file as `allowList`. Rust host: tools/paddock-dev/src/check/allow_list.rs (same
// entries, same findings; tools/paddock-dev/tests/it/check_allow_list.rs compares the error paths of every user on both hosts).
import { error, unwaivable, warn } from '../finding.mjs';

const UNREAD_FIX = 'Make the file readable (permissions) and run the check again.';

/** The entries of the allow list `file` (its JSON `text`): [{ path, site, reason, count }], or throws why it cannot be read. */
export function parseAllowList(text, file) {
  let v;
  try {
    v = JSON.parse(text);
  } catch (e) {
    throw new Error(`${file} is not JSON: ${e.message}`);
  }
  if (!Array.isArray(v?.allow)) throw new Error(`${file} has no \`allow\` array`);
  return v.allow.map((e) => {
    const path = typeof e?.path === 'string' ? e.path : '';
    const site = typeof e?.site === 'string' ? e.site.trim() : '';
    const reason = typeof e?.reason === 'string' ? e.reason.trim() : '';
    const count = e?.count === undefined ? 1 : e.count;
    if (!path || !site || [...reason].length < 8) {
      throw new Error(`${file}: every entry needs a path, a site (a piece of the flagged line) and a reason of at least 8 characters`);
    }
    if (!Number.isInteger(count) || count < 1) throw new Error(`${file}: \`count\` (${path}: ${site}) is a whole number of at least 1`);
    return { path, site, reason, count };
  });
}

/**
 * The allow list of one run of check `spec` ({ id, rule, file, unseen }) over `ctx`: `read` reads a file the check looks at, `admit`
 * answers per flagged site and counts, `warnings` names the stale entries and those for a
 * file the check does not read.
 */
export class AllowList {
  constructor(spec, entries, ctx) {
    this.spec = spec;
    this.file = spec.file;
    this.entries = entries;
    this.ctx = ctx;
    this.hits = entries.map(() => 0);
    this.scanned = new Set();
    this.unread = new Set();
  }

  /**
   * The text of `path`, a file the check looks at: { text }, or, when it cannot be read, { finding }: a not-measured error at that
   * file, which the check reports in its place (the file is then neither scanned nor a reason to call an entry stale).
   */
  read(path) {
    const text = this.ctx.read(path);
    if (text === null || text === undefined) {
      this.unread.add(path);
      const { id, rule, unseen } = this.spec;
      return { finding: unwaivable(error(id, rule, path, 1, `not measured: ${path} could not be read, so ${unseen} in it cannot be seen`, UNREAD_FIX), 'not-measured') };
    }
    this.scanned.add(path);
    return { text };
  }

  /** How many files the run read. */
  get probes() {
    return this.scanned.size;
  }

  /**
   * What the list says about one flagged site at `path`: `matches(site)` says whether an entry's `site` names it (C121 and C124: the
   * flagged line holds the site's text; C125: the literal starts with the site's whole path components). 'allowed' counts the site on
   * the first matching entry with room left; 'beyond' means entries name it but every one is at its count, and the caller reports it at
   * its own file and line; 'unlisted' means no entry names it.
   */
  admit(path, matches) {
    const named = this.entries.flatMap((a, i) => (a.path === path && matches(a.site) ? [i] : []));
    if (named.length === 0) return 'unlisted';
    const free = named.find((i) => this.hits[i] < this.entries[i].count);
    if (free === undefined) return 'beyond';
    this.hits[free]++;
    return 'allowed';
  }

  /**
   * The warnings for the entries that matched fewer sites than their count (`admit` never counts more). An entry is stale only for a file the run
   * read, or one that does not exist: an entry for a file the run never looked at, or could not read, is neither stale nor matched.
   * An entry for a file of the run's files that the check passed over (not its kind of file, or one of its own) names a file this
   * check does not read: it can never match, so it is a warning too.
   */
  warnings() {
    const { id, rule } = this.spec;
    const looked = (path) => this.scanned.has(path) || (!this.unread.has(path) && (this.ctx.read(path) === null || this.ctx.read(path) === undefined));
    const run = new Set(this.ctx.files());
    const out = [];
    this.entries.forEach((a, i) => {
      const hits = this.hits[i];
      if (run.has(a.path) && !this.scanned.has(a.path) && !this.unread.has(a.path)) {
        out.push(warn(id, rule, this.file, 1, `allow entry (${a.path}: ${a.site}) names a file this check does not read`, `Remove it from ${this.file}: no site in that file can need it.`));
      } else if (hits < a.count && looked(a.path)) {
        const what = hits === 0 ? 'matches no site' : `matches only ${hits} of its ${a.count} sites`;
        out.push(warn(id, rule, this.file, 1, `stale allow entry (${a.path}: ${a.site}) ${what}`, hits === 0 ? `Remove it from ${this.file}.` : `Lower its \`count\` to ${hits}.`));
      }
    });
    return out;
  }
}

/**
 * The allow list of check `spec` ({ id, rule, file, restore, unseen, parse }) read through `ctx`: { list, data }, or { broken }, the
 * check's whole result, when the file is missing, not JSON, without its list or with a malformed entry: one unwaivable check-broken
 * warning at the allow file (the file was the one thing looked at). `parse(text)` returns { entries, data } or throws why it cannot be
 * read (default: the shared parser, no data); `restore` is the fix of the broken finding, `unseen` what an unreadable file hides.
 */
export function readAllowList(ctx, spec) {
  const parse = spec.parse ?? ((text) => ({ entries: parseAllowList(text, spec.file) }));
  let parsed;
  try {
    const raw = ctx.read(spec.file);
    if (raw === null || raw === undefined) throw new Error(`${spec.file} cannot be read`);
    parsed = parse(raw);
  } catch (e) {
    return { broken: [unwaivable(warn(spec.id, spec.rule, spec.file, 1, `the check cannot look: ${e.message}`, spec.restore), 'check-broken')] };
  }
  return { list: new AllowList(spec, parsed.entries, ctx), data: parsed.data };
}
