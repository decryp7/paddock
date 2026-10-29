// The real guard of ADR 0022 §3.7 (plan P0.1b): Pacenotes opens hostile repositories and never runs cargo in them, so
// `paddock-repo-graph`'s `cargo-metadata` feature (which does) must be off in every build of this workspace. Ask cargo
// itself, with every feature on: `cargo tree -e features --all-features -i paddock-repo-graph` must not show the feature.
// "The package is not in the graph" is a pass. Conformance C52 is the backstop that reads the manifests.
import { execFileSync } from 'node:child_process';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const FEATURE = 'cargo-metadata';

/**
 * The verdict for a `cargo tree` run: `{ status, stdout, stderr }` (status 0 when it printed a tree).
 * @returns {{ ok: boolean, message: string }}
 */
export function verdict({ status, stdout = '', stderr = '' }) {
  if (status === 0) {
    return stdout.includes(FEATURE)
      ? { ok: false, message: `paddock-repo-graph's ${FEATURE} feature is enabled in this workspace:\n${stdout.trim()}` }
      : { ok: true, message: 'paddock-repo-graph is in the graph with the feature off' };
  }
  // cargo says so when no package of that name is in the graph (the stage-1 state until a crate depends on it).
  if (/did not match any packages|package ID specification .* did not match/.test(stderr)) {
    return { ok: true, message: 'paddock-repo-graph is not in the graph' };
  }
  return { ok: false, message: `cargo tree failed, so the guard cannot say the feature is off:\n${stderr.trim()}` };
}

/** Runs the guard in `cwd` with `run` (injected for tests). */
export function check(cwd, run = defaultRun) {
  return verdict(run(cwd));
}

function defaultRun(cwd) {
  try {
    const stdout = execFileSync('cargo', ['tree', '-e', 'features', '--all-features', '-i', 'paddock-repo-graph'], { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], timeout: 120_000 });
    return { status: 0, stdout, stderr: '' };
  } catch (e) {
    return { status: e.status ?? 1, stdout: String(e.stdout ?? ''), stderr: String(e.stderr ?? e.message) };
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const r = check(dirname(dirname(fileURLToPath(import.meta.url))));
  console.log(r.message);
  process.exit(r.ok ? 0 : 1);
}
