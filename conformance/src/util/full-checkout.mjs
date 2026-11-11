// A sparse checkout that lacks folders the gates read makes conformance, scripts/affected.mjs and `dev gates check` fail with a bare
// ENOENT, and agents then add folders one by one. This says it once, with the folders and the fix (ER-27). It compares the
// tracked folders (the top level, and the children of apps/) with the disk, so it needs no list of what the gates read.
import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { join } from 'node:path';

const git = (root, args) => execFileSync('git', ['-C', root, ...args], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] });

/** `null` when `root` is not a sparse checkout or has every tracked folder, else the `::error::` line to print. */
export function fullCheckoutProblem(root) {
  let folders;
  try {
    if (git(root, ['config', '--get', 'core.sparseCheckout']).trim() !== 'true') return null;
    folders = [...git(root, ['ls-tree', '-d', '--name-only', 'HEAD']).split('\n'), ...git(root, ['ls-tree', '-d', '--name-only', 'HEAD', 'apps/']).split('\n')];
  } catch {
    return null; // not a repository, no commit, or git is missing: nothing to judge
  }
  const missing = [...new Set(folders.map((f) => f.trim()).filter(Boolean))].filter((f) => !existsSync(join(root, f)));
  if (!missing.length) return null;
  return `::error::This checkout is sparse and lacks ${missing.join(', ')}, which the gates read. Fix: run \`git sparse-checkout disable\` (a full checkout costs about 1 s and 350 MB more than a cone), or \`git sparse-checkout add ${missing.join(' ')}\`. New worktrees from \`./dev worktree add\` are full checkouts.`;
}
