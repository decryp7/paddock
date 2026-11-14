// One allow list per check, one site per entry (ER-21; PHPStan's `reportUnmatchedIgnoredErrors` and the `count` of its baseline).
// An entry names a file (`path`), a piece of the flagged line (`site`), a `reason` of at least 8 characters and, when the same line
// appears more than once in the file, how many sites it covers (`count`, default 1). A whole file or folder is never allowed.
// After a run, an entry that matched fewer sites than its count is stale (remove it, or lower its count) and one that matched more is
// ambiguous (lengthen its `site`): both are warnings, so the list only ever names the sites that still exist.
// Used by C121 and C124. Rust host: tools/paddock-dev/src/check/allow_list.rs (same entries, same warnings).
import { warn } from '../finding.mjs';

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

/** The allow list of one run: `allows` answers per flagged site and counts, `warnings` names the stale and ambiguous entries. */
export class AllowList {
  constructor(file, entries) {
    this.file = file;
    this.entries = entries;
    this.hits = entries.map(() => 0);
  }

  /** Whether an entry names the site at `path` whose line is `text`; every entry that does counts the site. */
  allows(path, text) {
    let named = false;
    this.entries.forEach((a, i) => {
      if (a.path === path && String(text ?? '').includes(a.site)) {
        this.hits[i]++;
        named = true;
      }
    });
    return named;
  }

  /**
   * The warnings of check `id` (rule `rule`) for the entries that matched another number of sites than their count. `looked(path)` is
   * true when the run read that file or it does not exist: an entry for a file the run never looked at is neither stale nor matched.
   */
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

/** The allow list `file` read through `ctx`: an `AllowList`, or throws why it cannot be read. */
export function readAllowList(ctx, file) {
  const raw = ctx.read(file);
  if (raw === null || raw === undefined) throw new Error(`${file} cannot be read`);
  return new AllowList(file, parseAllowList(raw, file));
}
