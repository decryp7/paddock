// A runner that drives the Windows test VM for a whole run takes the interim VM lock once, at the start, so another agent cannot get in
// between two of its vm-win.sh calls (scripts/vm-lock.sh). Call `holdVmLock(label)` first in the entry point:
//   holdVmLock('run-win') ?? main(...)
// It returns undefined when this process already runs under a valid lock (the caller goes on), and otherwise runs the same command again
// under scripts/vm-lock-run.sh and exits with its code (75 when the lock is held, before anything starts).
import { spawnSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const RUN = join(dirname(fileURLToPath(import.meta.url)), '..', 'vm-lock-run.sh');

export function holdVmLock(label) {
  if (spawnSync('bash', [RUN, '--check'], { stdio: 'ignore' }).status === 0) return undefined;
  const r = spawnSync('bash', [RUN, label, '--', process.execPath, ...process.argv.slice(1)], { stdio: 'inherit', env: process.env });
  process.exit(r.status ?? 1);
}
