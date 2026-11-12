// One shrink-only rule for every baseline file (ER-12, ER-21): a baseline lists what is already wrong and may only get shorter. The
// entries a branch adds against its merge base with the target branch are an error; entries that are now fixed are only a warning
// ("remove it"), so two branches that each fix some of the same entries can land in either order. C71's three baselines and the
// baselines of the CI-env and shared-enum tests all use this.
import { execFileSync } from 'node:child_process';

/**
 * Matches `current` against `base` entries in two passes: first by `exact` (the full key, for example file + hash), then the leftovers
 * by `loose` (for example the hash alone). A pass-two match is a MOVED entry (same text, another file): not new, not stale. Counts are
 * per key, so two identical lines in two files never hide a real addition. Returns { fresh, stale, moved: [{ item, from }] }; with no
 * `loose` nothing is moved.
 */
export function reconcile(current, base, exact, loose) {
  const push = (m, k, v) => (m.get(k) ?? m.set(k, []).get(k)).push(v);
  const buckets = new Map(); // exact key -> indexes into `base`, in order
  base.forEach((e, i) => push(buckets, exact(e), i));
  const unmatched = new Set(base.keys());
  const rest = [];
  for (const c of current) {
    const hit = buckets.get(exact(c))?.shift();
    if (hit === undefined) rest.push(c);
    else unmatched.delete(hit);
  }
  const pool = new Map(); // loose key -> unmatched indexes
  if (loose) for (const i of unmatched) push(pool, loose(base[i]), i);
  const fresh = [];
  const moved = [];
  for (const c of rest) {
    const i = loose ? pool.get(loose(c))?.shift() : undefined;
    if (i === undefined) fresh.push(c);
    else {
      unmatched.delete(i);
      moved.push({ item: c, from: base[i] });
    }
  }
  return { fresh, stale: [...unmatched].sort((a, b) => a - b).map((i) => base[i]), moved };
}

const defaultGit = (cwd) => (args) => execFileSync('git', args, { cwd, encoding: 'utf8', maxBuffer: 64 << 20, stdio: ['ignore', 'pipe', 'pipe'] });

/**
 * The entries of `current` that the baseline at the merge base does not have (a multiset difference by `keyOf`), or `{ reason }` when
 * it cannot be measured (no git, no merge base, an unreadable file). A baseline absent at the merge base is the first introduction:
 * nothing was added. With `looseKeyOf` an entry whose loose key (say the hash) is in the base but whose full key is not is `moved`, not
 * added (`moved: [{ item, from }]`). `entriesOf(json)` reads the list out of the parsed file; `git` and `baseRef` are injectable for tests.
 */
export function baselineGrowth({ root, path, current, keyOf, entriesOf, git = defaultGit(root), baseRef = process.env.PADDOCK_BASE_REF ?? 'origin/main', looseKeyOf }) {
  let base;
  let text;
  try {
    const head = git(['rev-parse', 'HEAD']).trim();
    base = git(['merge-base', 'HEAD', baseRef]).trim();
    if (base === head) base = git(['rev-parse', 'HEAD^1']).trim(); // on the target branch itself: this commit against its parent
  } catch (e) {
    return { reason: `no merge base (${String(e.message ?? e).split('\n')[0].slice(0, 120)})` };
  }
  try {
    text = git(['show', `${base}:${path}`]);
  } catch (e) {
    if (/does not exist|exists on disk, but not in|not in/.test(String(e.stderr ?? e.message))) return { base, added: [] };
    return { reason: `the baseline at ${base.slice(0, 9)} cannot be read` };
  }
  let entries;
  try {
    entries = entriesOf(JSON.parse(text));
  } catch {
    return { reason: `the baseline at ${base.slice(0, 9)} cannot be parsed` };
  }
  const { fresh, moved } = reconcile(current, entries, keyOf, looseKeyOf);
  return { base, added: fresh, moved };
}

/**
 * For a test over a baseline: throws when entries were added against the merge base, warns (never fails) when the merge base cannot be
 * read or when listed entries are gone. `gone` is the list of entries that no longer match anything.
 */
export function enforceShrinkOnly({ path, current, gone = [], ...rest }) {
  const g = baselineGrowth({ path, current, ...rest });
  if (g.reason) console.warn(`${path}: the shrink-only rule is not enforced here: ${g.reason}`);
  else if (g.added.length) {
    throw new Error(`${path} grew: ${g.added.length} entr${g.added.length === 1 ? 'y is' : 'ies are'} not in the baseline at the merge base ${g.base.slice(0, 9)} (${g.added.slice(0, 5).map((e) => rest.keyOf(e)).join('; ')}). It may only shrink: fix the sites instead.`);
  }
  if (gone.length) console.warn(`${path}: ${gone.length} entr${gone.length === 1 ? 'y is' : 'ies are'} fixed or gone; remove ${gone.slice(0, 5).join('; ')}`);
  return g;
}
