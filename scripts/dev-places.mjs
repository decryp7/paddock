// Where this machine keeps its development folders, derived and never named (C125, rule 40). The layout is described once, in
// docs/dev-setup.md ("Where things live"); the Rust original is tools/paddock-dev/src/places.rs and this is a registered copy
// (docs/known-copies.toml, `dev-places`), as are scripts/dev-places.sh and .claude/hooks/lib/paddockpaths.py.
//
// devTargets(env)   PADDOCK_DEV_TARGETS, else the parent of `git config paddock.worktreeRoot`, else of PADDOCK_WORKTREE_ROOT
// devDisk(env)      the folder that holds the dev-targets folder (the dev disk's root)
// vmData(env)       PADDOCK_VM_DATA, else <dev disk>/vm-data
// vmDataPinned()    <dev disk>/vm-data from the repository's own git config only, read with an empty environment: for the
//                   guards no variable may move (win-vm-target.mjs)
// Each throws an Error naming the next step when it cannot be derived (fail closed: never a guessed disk).
import { execFileSync } from 'node:child_process';
import { dirname, isAbsolute, posix } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const HINT = 'run `git config paddock.worktreeRoot <dev-targets>/wt` once in the main checkout, or set PADDOCK_DEV_TARGETS (docs/dev-setup.md)';
const PINNED_PATH = '/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin';

/** The worktree root in the repository's git config, or '' (git missing, not a checkout, not set). */
export function gitWorktreeRoot({ pinned = false, env = process.env } = {}) {
  const args = ['-C', HERE, 'config', ...(pinned ? ['--local'] : []), '--get', 'paddock.worktreeRoot'];
  try {
    return execFileSync('git', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'], timeout: 10_000, env: pinned ? { PATH: PINNED_PATH } : env }).trim();
  } catch {
    return '';
  }
}

/** The parent of an absolute folder below the filesystem root, or throws. */
export function parentOf(path, what) {
  const p = String(path).replace(/\/+$/, '');
  if (!isAbsolute(p) || !p.startsWith('/')) throw new Error(`dev-places: ${what} '${path}' is not an absolute path; ${HINT}`);
  const parent = posix.dirname(p);
  if (parent === '/' || parent === p) throw new Error(`dev-places: ${what} '${path}' has no parent folder; ${HINT}`);
  return parent;
}

/** The pure rule: the named folder, else the parent of the worktree root. */
export function devTargetsOf({ named, worktreeRoot }) {
  if (named) {
    if (!named.startsWith('/')) throw new Error(`dev-places: PADDOCK_DEV_TARGETS '${named}' is not an absolute path; ${HINT}`);
    return named.replace(/\/+$/, '');
  }
  if (!worktreeRoot) throw new Error(`dev-places: the dev-targets folder cannot be derived: no worktree root is set; ${HINT}`);
  return parentOf(worktreeRoot, 'the worktree root');
}

export function devTargets(env = process.env, root = () => gitWorktreeRoot({ env }) || env.PADDOCK_WORKTREE_ROOT || '') {
  return devTargetsOf({ named: env.PADDOCK_DEV_TARGETS || '', worktreeRoot: env.PADDOCK_DEV_TARGETS ? '' : root() });
}

export function devDisk(env = process.env, root) {
  return parentOf(devTargets(env, root), 'the dev-targets folder');
}

export function vmData(env = process.env, root) {
  if (env.PADDOCK_VM_DATA) return env.PADDOCK_VM_DATA.replace(/\/+$/, '');
  return `${devDisk(env, root)}/vm-data`;
}

/** <dev disk>/vm-data from a worktree root read from the repository alone (`pinned`); `worktreeRoot` is injected by tests. */
export function vmDataPinned(worktreeRoot = gitWorktreeRoot({ pinned: true })) {
  if (!worktreeRoot) throw new Error('dev-places: the VM folder cannot be derived: the repository\'s git config has no paddock.worktreeRoot; run `git config paddock.worktreeRoot <dev-targets>/wt` once in the main checkout (docs/dev-setup.md)');
  return `${parentOf(parentOf(worktreeRoot, 'the worktree root'), 'the dev-targets folder')}/vm-data`;
}
