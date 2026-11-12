// Run before the tests (npm test): several of them read the repository around conformance/ (.gitea, tools), so in a sparse
// worktree they fail deep inside with a missing file. Say so once, in one line, and stop. It looks at the paths, never at the
// `core.sparseCheckout` key: the host kept clones of CI set that key (non-cone) and still have every folder.
import { existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

export const SPARSE_MESSAGE = 'sparse worktree: run `git sparse-checkout disable` or use a full worktree';
const NEEDED = ['tools/paddock-dev/Cargo.toml', 'conformance/package.json', '.gitea/workflows/ci.yml'];

/** True when a folder the tests read is missing from the repository at `root`. */
export function isSparse(root) {
  return NEEDED.some((p) => !existsSync(join(root, p)));
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const root = process.argv[2] ?? join(dirname(fileURLToPath(import.meta.url)), '..', '..');
  if (isSparse(root)) {
    console.error(`::error::${SPARSE_MESSAGE}`);
    process.exit(1);
  }
}
