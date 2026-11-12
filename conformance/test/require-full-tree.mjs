// Run before the tests (npm test): several of them read the repository around conformance/ (.gitea, tools, crates), so in a
// sparse worktree they fail deep inside with a missing file. Say so once, in one line, and stop.
import { execFileSync } from 'node:child_process';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

export const SPARSE_MESSAGE = 'sparse worktree: run `git sparse-checkout disable` or use a full worktree';

/** True when the checkout that holds `dir` is a sparse one. */
export function isSparse(dir) {
  try {
    return execFileSync('git', ['-C', dir, 'config', '--get', 'core.sparseCheckout'], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim() === 'true';
  } catch {
    return false; // not set, or not a git checkout (an exported tree): nothing to say
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const dir = process.argv[2] ?? dirname(fileURLToPath(import.meta.url));
  if (isSparse(dir)) {
    console.error(`::error::${SPARSE_MESSAGE}`);
    process.exit(1);
  }
}
