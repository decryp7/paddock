// Finds the Paddock checkout that holds the shared sources (templates/repo, docs/blocks, lint, renovate). A check that
// needs them gets it from `--paddock <dir>`, or from the checkout this package sits in; when neither exists it says so.
import { existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));

/** The Paddock root for a run (`options.paddock`, validated), or null when none was given or it lacks templates/repo. */
export function paddockRoot(options = {}) {
  return options.paddock && existsSync(join(options.paddock, 'templates', 'repo', 'manifest.json')) ? options.paddock : null;
}

/** The checkout this package sits in (conformance/src/sync/../../..), when it is one; the CLI's default for `--paddock`. */
export function ownPaddock() {
  const d = resolve(here, '..', '..', '..');
  return existsSync(join(d, 'templates', 'repo', 'manifest.json')) ? d : undefined;
}
