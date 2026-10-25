// Whether a module is the script Node was started with. Node gives the main module's `import.meta.url` its real path (symlinks
// resolved) but leaves `process.argv[1]` as it was given, so comparing the two directly says "no" whenever the script is reached
// through a link: a CI work folder linked to a stable one (stable-workspace), a package in node_modules, a symlinked home. The
// script then exits 0 without doing anything. Every script's entry point goes through this check (ci-actions.test.mjs).
import { realpathSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

/** True when `metaUrl` (a module's `import.meta.url`) is the file `argv1` (default `process.argv[1]`) names, links resolved. */
export function isMain(metaUrl, argv1 = process.argv[1]) {
  if (!argv1) return false;
  try {
    return realpathSync(resolve(argv1)) === realpathSync(fileURLToPath(metaUrl));
  } catch {
    return false;
  }
}
