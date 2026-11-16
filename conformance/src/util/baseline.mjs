// One shrink-only rule for every baseline file (ER-12, ER-21): a baseline lists what is already wrong and may only get shorter. The
// entries a branch adds against its merge base with the target branch are an error; entries that are now fixed are only a warning
// ("remove it"), so two branches that each fix some of the same entries can land in either order. C71's three baselines and the
// baselines of the CI-env and shared-enum tests all use this.
import { execFileSync } from 'node:child_process';
import { error, unwaivable, warn } from '../finding.mjs';

/**
 * Matches `current` against `base` entries in two passes: first by `exact` (the full key, for example file + hash), then the leftovers
 * by `loose` (for example the hash alone). A pass-two match is a MOVED entry (same text, another file): not new, not stale. Counts are
 * per key, so two identical lines in two files never hide a real addition. Returns { fresh, stale, moved: [{ item, from }] }; with no
 * `loose` nothing is moved; `canMove(item, from)` can refuse a pairing (the site check lets only an entry whose file is gone move).
 */
export function reconcile(current, base, exact, loose, canMove = () => true) {
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
    const list = loose ? pool.get(loose(c)) : undefined;
    const at = list ? list.findIndex((j) => canMove(c, base[j])) : -1;
    if (at < 0) fresh.push(c);
    else {
      const i = list.splice(at, 1)[0];
      unmatched.delete(i);
      moved.push({ item: c, from: base[i] });
    }
  }
  return { fresh, stale: [...unmatched].sort((a, b) => a - b).map((i) => base[i]), moved };
}

/**
 * The ref every baseline's and allow list's growth is measured against: the run's own (`options.baseRef`), else PADDOCK_BASE_REF, else
 * `origin/main`; an empty value is unset. The ONE place that reads PADDOCK_BASE_REF (ER-21: C53, C71, C115, C104 and the shared
 * allow-list reader each read it or forgot it, and a member branch of a train was measured against main, so the train's own entries
 * counted as its growth). CI's conformance and train-gates jobs and `./dev check --pre-push` set it to the branch's target (the train
 * before, for a train member). A deliberate change of meaning for the blocking baselines (C53, C71, C104, C115): the target branch
 * instead of `origin/main` (ADR 0098, amendment). Rust host: `check::baseline::target_ref`.
 */
export const targetRef = (options) => options?.baseRef || process.env.PADDOCK_BASE_REF || 'origin/main';

/** Whether the run is a CI run (`CI`, `GITEA_ACTIONS` or `GITHUB_ACTIONS` set and not false/0): a target that cannot be seen is then not measured. */
export const isCi = (env = process.env) => ['CI', 'GITEA_ACTIONS', 'GITHUB_ACTIONS'].some((k) => env[k] && !/^(?:false|0)$/i.test(env[k]));

const FETCH_FIX = 'Fetch the target branch (`git fetch origin main`, full history), or set PADDOCK_BASE_REF to the ref to compare with.';
const PARSE_FIX = 'The list at the base is in an older format (an entry without a `site`), so growth cannot be measured against it; nothing to fix here: it is measured once the new format is on the base branch.';

/**
 * The finding for a growth check that could not measure (`g` = { reason, kind } from `baselineGrowth`): a warning with the next step,
 * except that in CI a target that cannot be resolved is a not-measured error no waiver covers (ADR 0089: not measured never passes;
 * a blocking baseline must not turn into a warning because the target ref is missing). `what` names the list ("the baseline").
 * `ctx.options.ci` overrides the environment (tests).
 */
export function unmeasuredFinding(ctx, { id, rule, file, line = 1, what = 'the baseline', g }) {
  if (g.kind === 'no-base' && (ctx.options?.ci ?? isCi())) {
    return unwaivable(error(id, rule, file, line, `not measured: ${what}'s shrink-only rule cannot be checked, the target ref is not usable: ${g.reason}`, `${FETCH_FIX} A gate that cannot see its target does not pass (ADR 0089).`), 'not-measured');
  }
  return warn(id, rule, file, line, `${what}'s shrink-only rule is not enforced here: ${g.reason}`, g.kind === 'unparsed' ? PARSE_FIX : FETCH_FIX);
}

const defaultGit = (cwd) => (args) => execFileSync('git', args, { cwd, encoding: 'utf8', maxBuffer: 64 << 20, stdio: ['ignore', 'pipe', 'pipe'] });

/** Whether `root` is a git work tree git itself accepts (a folder with a stray `.git` file, as a fixture has, is not). */
export function isWorkTree(root, git = defaultGit(root)) {
  try {
    return git(['rev-parse', '--is-inside-work-tree']).trim() === 'true';
  } catch {
    return false;
  }
}

/**
 * The entries of `current` that the baseline at the merge base does not have (a multiset difference by `keyOf`), or `{ reason }` when
 * it cannot be measured (no git, no merge base, an unreadable file). A baseline absent at the merge base is the first introduction:
 * nothing was added. With `looseKeyOf` an entry whose loose key (say the hash) is in the base but whose full key is not is `moved`, not
 * added (`moved: [{ item, from }]`). `entriesOf(json)` reads the list out of the parsed file; `git` and `baseRef` are injectable for tests.
 */
export function baselineGrowth({ root, path, current, keyOf, entriesOf, git = defaultGit(root), baseRef = targetRef(), looseKeyOf }) {
  let base;
  let text;
  try {
    const head = git(['rev-parse', 'HEAD']).trim();
    base = git(['merge-base', 'HEAD', baseRef]).trim();
    if (base === head) base = git(['rev-parse', 'HEAD^1']).trim(); // on the target branch itself: this commit against its parent
  } catch (e) {
    return { kind: 'no-base', reason: `no merge base (${String(e.message ?? e).split('\n')[0].slice(0, 120)})` };
  }
  try {
    text = git(['show', `${base}:${path}`]);
  } catch (e) {
    if (/does not exist|exists on disk, but not in|not in/.test(String(e.stderr ?? e.message))) return { base, added: [], moved: [] };
    return { kind: 'unreadable', reason: `the baseline at ${base.slice(0, 9)} cannot be read` };
  }
  let entries;
  try {
    entries = entriesOf(JSON.parse(text));
  } catch {
    return { kind: 'unparsed', reason: `the baseline at ${base.slice(0, 9)} cannot be parsed` };
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
