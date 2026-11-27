// One shrink-only rule for every baseline file (ER-12, ER-21): a baseline lists what is already wrong and may only get shorter. The
// entries a branch adds against its merge base with the target branch are an error; entries that are now fixed are only a warning
// ("remove it"), so two branches that each fix some of the same entries can land in either order. C71's three baselines and the
// baselines of the CI-env and shared-enum tests all use this.
// A check's list has to land on the target branch while the check is still report-only: a list that does not exist at the merge base is an empty base, so every entry
// of a list created on a branch is growth (ER-46) and the branch gets one finding per file until the list is on the target branch.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
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
 * counted as its growth). CI's conformance job and `./dev check --pre-push` set it to the branch's target (the train
 * before, for a train member). A deliberate change of meaning for the blocking baselines (C53, C71, C104, C115): the target branch
 * instead of `origin/main` (ADR 0098, amendment). Rust host: `check::baseline::target_ref`.
 */
export const targetRef = (options) => options?.baseRef || process.env.PADDOCK_BASE_REF || 'origin/main';
/** The variable's name, for a check that reads a workflow (declared data) for it; the value is read only by `targetRef`. */
export const BASE_REF_VAR = 'PADDOCK_' + 'BASE_REF';

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

/** The register of the user's approvals of growth (ER-46): docs/gates.toml. Rust host: `check::growth_approval` (the same entries, the same digest). */
export const GROWTH_REGISTER = 'docs/gates.toml';

/** SHA-256 (hex) of the added keys, sorted by bytes and joined by a newline: what an approval of growth names. */
export function growthDigest(keys) {
  const sorted = keys.map((k) => Buffer.from(k)).sort(Buffer.compare).map((b) => b.toString('utf8'));
  return createHash('sha256').update(sorted.join('\n')).digest('hex');
}

/** The `[[register_growth]]` tables of the register's text: string and integer values only (as paddock.waivers.toml is read). */
export function registerGrowth(text) {
  const out = [];
  let cur = null;
  for (const raw of String(text ?? '').split(/\r?\n/)) {
    const line = raw.trim();
    if (line.startsWith('[')) {
      cur = line === '[[register_growth]]' ? {} : null;
      if (cur) out.push(cur);
    } else if (cur && line && !line.startsWith('#')) {
      const m = /^([A-Za-z_]+)\s*=\s*(?:"((?:[^"\\]|\\.)*)"|(-?\d+))\s*(?:#.*)?$/.exec(line);
      if (m) cur[m[1]] = m[3] === undefined ? m[2] : Number(m[3]);
    }
  }
  return out;
}

/** Whether an entry of the user in `text` (docs/gates.toml) names exactly these additions of `check`: count and digest. */
export function growthApproved(text, check, keys) {
  const digest = growthDigest(keys);
  return registerGrowth(text).some(
    (e) => e.check === check && e.by === 'user' && typeof e.date === 'string' && e.date.trim() !== '' && e.count === keys.length && e.digest === digest,
  );
}

/** The line the user adds to the register to approve `keys` for `check` (the finding's fix prints it). */
export const approvalLine = (check, keys) =>
  `The user records it in ${GROWTH_REGISTER}: [[register_growth]] check = "${check}", by = "user", date = "<day>", count = ${keys.length}, digest = "${growthDigest(keys)}" (ER-46).`;

/**
 * The growth `g` of check `check` after the user's approval (`register` is the register's text, read from the tree when omitted): approved
 * additions are dropped (`added` empty); otherwise `g.ask` is the sentence for the fix. A growth without a `check` is returned as it is.
 */
function approve(g, { root, check, register, keyOf }) {
  if (!check || !g.added?.length) return g;
  const keys = g.added.map(keyOf);
  let text = register;
  if (text === undefined) {
    try {
      text = readFileSync(join(root, GROWTH_REGISTER), 'utf8');
    } catch {
      text = '';
    }
  }
  return growthApproved(text, check, keys) ? { ...g, added: [] } : { ...g, ask: approvalLine(check, keys) };
}

/** The merge base of HEAD and `baseRef`; on the target branch itself, the commit's first parent. Throws when git cannot say. */
function mergeBaseOf(git, baseRef) {
  const head = git(['rev-parse', 'HEAD']).trim();
  const base = git(['merge-base', 'HEAD', baseRef]).trim();
  return base === head ? git(['rev-parse', 'HEAD^1']).trim() : base; // on the target branch itself: this commit against its parent
}

/**
 * The files git's own rename detection pairs between `base` and the work tree (committed, staged or not): a Map of old path to new path,
 * read from `git diff -M --name-status -z` (its NUL-separated format, split, not matched: ER-42). A register keyed by path follows a
 * rename (an entry counts for the new path; ER-46: not growth). Empty when git cannot say: the stricter reading. Rust host:
 * `renames` in tools/paddock-dev/src/check/baseline.rs.
 */
export function renamedFiles(git, base) {
  const map = new Map();
  let out;
  try {
    out = git(['diff', '-M', '--diff-filter=R', '--name-status', '-z', '--no-color', '--no-ext-diff', base]);
  } catch {
    return map;
  }
  const t = out.split('\0');
  for (let i = 0; i + 2 < t.length; i += 3) if (t[i].startsWith('R')) map.set(t[i + 1], t[i + 2]);
  return map;
}

/** The renames ([`renamedFiles`]) between the merge base with `baseRef` and the work tree of `root`; empty when git cannot give a merge base. */
export function renamesSinceBase({ root, git = defaultGit(root), baseRef = targetRef() }) {
  try {
    return renamedFiles(git, mergeBaseOf(git, baseRef));
  } catch {
    return new Map();
  }
}

/**
 * The entries of `current` that the baseline at the merge base does not have (a multiset difference by `keyOf`), or `{ reason }` when
 * it cannot be measured (no git, no merge base, an unreadable file). A baseline absent at the merge base is an empty base (every entry is growth, ER-46).
 * With `looseKeyOf` an entry whose loose key (say the hash) is in the base but whose full key is not is `moved`, not
 * added (`moved: [{ item, from }]`). With `rekey(item, renames)` an entry takes the path git's rename detection gives its file now, on the
 * merge base's side and the current one alike, so an entry that follows a pure move is the same entry (a register keyed by path). With
 * `check`, additions the user approved in the register are dropped and the rest carry `ask`. `entriesOf(json)` reads the list out of the
 * parsed file; `git` and `baseRef` are injectable for tests.
 */
export function baselineGrowth({ root, path, current, keyOf, entriesOf, git = defaultGit(root), baseRef = targetRef(), looseKeyOf, rekey, check, register }) {
  const done = (g) => approve(g, { root, check, register, keyOf });
  let base;
  let text;
  try {
    base = mergeBaseOf(git, baseRef);
  } catch (e) {
    return { kind: 'no-base', reason: `no merge base (${String(e.message ?? e).split('\n')[0].slice(0, 120)})` };
  }
  try {
    text = git(['show', `${base}:${path}`]);
  } catch (e) {
    // A list absent at the merge base is an empty base: every current entry is growth (ER-46). Treating it as the "first introduction" let a new list with any
    // number of entries in silently (a new apps/<app>/app/eslint-suppressions.json with count 5, the second time this class was found).
    if (/does not exist|exists on disk, but not in|not in/.test(String(e.stderr ?? e.message))) {
      const { fresh, moved } = reconcile(current, [], keyOf, looseKeyOf);
      return done({ base, added: fresh, moved });
    }
    return { kind: 'unreadable', reason: `the baseline at ${base.slice(0, 9)} cannot be read` };
  }
  let entries;
  try {
    entries = entriesOf(JSON.parse(text));
  } catch {
    return { kind: 'unparsed', reason: `the baseline at ${base.slice(0, 9)} cannot be parsed` };
  }
  if (rekey) {
    const map = renamedFiles(git, base);
    current = current.map((e) => rekey(e, map));
    entries = entries.map((e) => rekey(e, map));
  }
  const { fresh, moved } = reconcile(current, entries, keyOf, looseKeyOf);
  return done({ base, added: fresh, moved });
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
