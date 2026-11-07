// Where the shared screenshot rules (paddock-e2e's screenshot-check.ts, the one implementation) are loaded from, and what a round that cannot load them is told.
// They need two things in the checkout the harness runs from: the `ui/` folder (a sparse worktree without it fails) and the repo's node_modules (`npm ci` at the
// repo root: the module imports @playwright/test). A round therefore needs a FULL checkout, or a sparse one that includes `ui` and `package.json`; the harness
// says so instead of failing with a bare "Cannot find module". PADDOCK_SCREENSHOT_RULES points at another copy of the file.
import { existsSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
export const RULES_PATH = process.env.PADDOCK_SCREENSHOT_RULES ?? resolve(HERE, '..', '..', 'ui', 'projects', 'paddock-e2e', 'src', 'screenshot-check.ts');

/** The message for a rules module that cannot be loaded: what is missing and the next step. Pure. */
export function rulesProblem(path, { exists, error = '' }) {
  if (!exists) return `the shared screenshot rules are not in this checkout (${path} is missing): the round needs a full checkout of the repository (a sparse worktree must include ui/), or PADDOCK_SCREENSHOT_RULES set to a copy`;
  if (/Cannot find (package|module) '?(@playwright|playwright)/.test(error)) return `the shared screenshot rules need the repository's node_modules (${error.split('\n')[0]}): run \`npm ci\` at the repository root of this checkout first`;
  return `the shared screenshot rules could not be loaded: ${error.split('\n')[0]}`;
}

/** Imports the rules module; throws an Error whose message is `rulesProblem`. */
export async function importScreenshotRules(path = RULES_PATH) {
  if (!existsSync(path)) throw new Error(rulesProblem(path, { exists: false }));
  try { return await import(path); } catch (e) { throw new Error(rulesProblem(path, { exists: true, error: String(e.message ?? e) })); }
}
