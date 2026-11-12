// Owner record for a folder a vm-smoke run made (`scripts/disk-record.sh` -> `./dev disk record`, ER-2 owner records). Never fails the
// run: it is bounded (20 s by default) and a failure or timeout leaves one ::warning:: line with the cause and the next step (ER-27).
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';

/** Records `dir` as a `scratch` folder of the repository `repo`; returns true when the script ran and exited 0. */
export function recordScratch(dir, repo, { spawn = spawnSync, log = console.error, timeoutMs = 20_000 } = {}) {
  const r = spawn('bash', [join(repo, 'scripts', 'disk-record.sh'), dir, 'scratch'], { cwd: repo, stdio: 'ignore', timeout: timeoutMs });
  if (r.status === 0 && !r.error) return true;
  const why = r.error ? `${r.error.code ?? r.error.message} after ${timeoutMs / 1000} s` : `exit ${r.status}`;
  log(`::warning::vm-smoke: owner record for ${dir} failed (${why}); the run goes on. Next step: run \`./dev disk record ${dir} --kind scratch --link <worktree>\` by hand.`);
  return false;
}
