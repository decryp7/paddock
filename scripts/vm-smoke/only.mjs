// `--only <row ids>` of the macOS runner (run.mjs): run just the open rows, plus the install, the build binding, the cold start and the final Quit they need.
// Pure functions: the ids are the manifest's row ids, so an unknown id fails at once instead of running nothing.
import { requiredSteps } from './manifest.mjs';

/** The rows `--only` can pick on macOS (the power suite's release rows driven by `macRows`). */
export const MAC_ONLY_ROWS = ['mac-reopen', 'mac-lifecycle-log', 'hung-page', 'idle-action', 'helper-on', 'login-jobs', 'jobs-restart', 'jobs-stopped', 'upgrade-swap', 'disk-space'];
/** The steps every `--only` run keeps: nothing is installed or started without them. */
export const MAC_ONLY_BASE = ['install', 'build-binding', 'cold-start', 'quit'];

/** Parse `a,b` into a list of known, distinct ids. Throws a message naming the fix. */
export function parseOnly(value) {
  const ids = String(value ?? '').split(',').map((s) => s.trim()).filter(Boolean);
  if (ids.length === 0) throw new Error('--only needs row ids, for example --only mac-reopen');
  for (const id of ids) if (!MAC_ONLY_ROWS.includes(id)) throw new Error(`--only ${id}: unknown row (macOS: ${MAC_ONLY_ROWS.join(', ')})`);
  return [...new Set(ids)];
}

/** Required steps of an `--only` run: the base steps and the picked rows, taken from the power manifest. */
export function onlyRequired(ids) {
  const all = requiredSteps({ os: 'macos', suite: 'power', release: true });
  const want = new Set([...MAC_ONLY_BASE, ...ids, ...(ids.includes('helper-on') ? ['helper-off'] : [])]); // Turn on is always paired with Turn off
  const picked = all.filter((r) => want.has(r.id));
  const missing = [...want].filter((id) => !picked.some((r) => r.id === id));
  if (missing.length) throw new Error(`the manifest has no row ${missing.join(', ')}`);
  return picked;
}

/** True when a row runs: always without `--only`, else only when picked. */
export const rowWanted = (only, id) => !only || only.includes(id);
