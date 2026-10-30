// C57: no tracked file is a symlink (git mode 120000), except those on the allow-list (empty today). A symlink in the tree
// points at a path of one machine: it breaks on a clean runner and on Windows checkouts. The case that prompted it: the
// root `.gitignore` entry `node_modules/` cannot match a symlink, so a `node_modules` link was committed (83d6437fe).
// Reads `git ls-files -s -- .` in the checked root (the repo or a folder inside it); a submodule (mode 160000) is not a symlink;
// not a git checkout: nothing to check.
import { execFileSync } from 'node:child_process';
import { error } from '../finding.mjs';

/** Tracked paths that may be symlinks (exact paths, relative to the checked root). Keep empty unless a link is needed. */
export const ALLOWED_SYMLINKS = [];

/** Paths of the symlinks in `git ls-files -s` output (mode 120000), unless allowed. */
export function trackedSymlinks(lsFilesStage, allowed = ALLOWED_SYMLINKS) {
  return lsFilesStage
    .split('\n')
    .map((l) => /^120000 \S+ \d\t(.+)$/.exec(l)?.[1])
    .filter((p) => p !== undefined && !allowed.includes(p));
}

export default {
  id: 'C57',
  title: 'No tracked file is a symlink',
  rule: 40,
  run(ctx) {
    // The repository may sit above the checked root (an app folder): `-- .` lists only the tracked files below it.
    const git = ctx.options.git ?? ((args) => execFileSync('git', args, { cwd: ctx.root, encoding: 'utf8', maxBuffer: 64 << 20 }));
    let listing;
    try {
      git(['rev-parse', '--show-toplevel']);
      listing = git(['ls-files', '-s', '--', '.']);
    } catch {
      return []; // not a git checkout (an export): nothing to check
    }
    return trackedSymlinks(listing).map((p) =>
      error('C57', 40, p, 1, `${p} is a tracked symlink (git mode 120000): it points at one machine's path`, 'Remove it from git (`git rm --cached`), ignore it without a trailing slash (`node_modules`, not `node_modules/`), or add it to ALLOWED_SYMLINKS in C57 with the reason.'),
    );
  },
};
