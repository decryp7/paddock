// The runners' door to the VM broker (`./dev vm`, tools/paddock-dev/src/vm, ADR 0054 §1.5). Every start, revert, stop and .vmx edit of a
// test VM goes through here; no runner calls vmrun, tart (run, clone, delete, stop, set) or writes a .vmx (check C97).
//
//   const lease = Lease.take(REPO, 'paddock-win11-test', 'vm-smoke win dyno');   // refused: BrokerError (kind 'refused')
//   lease.revert('gui-session-tools'); lease.start(); ...; lease.stop(); lease.release();
//
// The lease is held by this process (pid and start time), so it is gone if the runner dies. A refusal or a failed preflight is
// `could not run`, never a product failure: `BrokerError.couldNotRun` is true for exit 4.
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';

export class BrokerError extends Error {
  constructor(message, { kind = 'failure', hint = '', code = 1 } = {}) {
    super(hint ? `${message} (${hint})` : message);
    this.kind = kind;
    this.code = code;
  }

  /** Exit 4: a lease is held, a guest runs without one, or the preflight failed. */
  get couldNotRun() {
    return this.kind === 'refused';
  }
}

/** The binary: PADDOCK_DEV (a built paddock-dev), else the repository's `./dev` wrapper. */
export const devPath = (repo) => process.env.PADDOCK_DEV ?? join(repo, 'dev');

/** Runs `dev --json vm <args>` and returns the parsed JSON object; throws BrokerError on a non-zero exit. */
export function vm(repo, args, { timeoutMs = 900_000, spawn = spawnSync } = {}) {
  const r = spawn(devPath(repo), ['--json', 'vm', ...args], { encoding: 'utf8', timeout: timeoutMs, env: process.env });
  if (r.error) throw new BrokerError(`could not run ${devPath(repo)}: ${r.error.message}`);
  let data = null;
  try {
    data = JSON.parse(String(r.stdout ?? '').trim());
  } catch {
    // not JSON: the message below carries stderr
  }
  const code = r.status ?? (r.signal ? 124 : 1);
  if (code !== 0 || !data || data.ok === false) {
    const msg = data?.error ?? (String(r.stderr ?? '').trim().split('\n').pop() || `dev vm ${args[0]} exited ${code}`);
    throw new BrokerError(msg, { kind: data?.kind ?? 'failure', hint: data?.hint ?? '', code });
  }
  return data;
}

/** Whether `vmName` runs, and its lease (no lease needed). */
export const status = (repo, vmName, opts) => vm(repo, ['status', vmName], { timeoutMs: 120_000, ...opts });

export class Lease {
  constructor(repo, id, vmName, opts = {}) {
    this.repo = repo;
    this.id = id;
    this.vm = vmName;
    this.opts = opts;
  }

  /** Takes the lease on `vmName` for this process. `waitSec` queues behind a holder; `adopt` is for a person stopping a guest they started. */
  static take(repo, vmName, purpose, { waitSec = 0, patienceSec, adopt = false, holderPid = process.pid, ...opts } = {}) {
    const args = ['lease', vmName, '--for', purpose, '--holder-pid', String(holderPid), '--wait', String(waitSec)];
    if (patienceSec) args.push('--patience', String(patienceSec));
    if (adopt) args.push('--adopt');
    const d = vm(repo, args, { timeoutMs: (waitSec + 120) * 1000, ...opts });
    return new Lease(repo, d.lease, vmName, opts);
  }

  call(args, timeoutMs) {
    return vm(this.repo, [args[0], this.id, ...args.slice(1)], { timeoutMs, ...this.opts });
  }

  preflight() { return this.call(['preflight'], 120_000); }
  /** Preflight, headless start, wait until the hypervisor reports it running. `args` go to `tart run`; `maxLoad` bounds the host load. */
  start({ args = [], maxLoad } = {}) {
    const a = ['start', ...args.flatMap((x) => ['--arg', x]), ...(maxLoad ? ['--max-load', String(maxLoad)] : [])];
    return this.call(a, 900_000);
  }
  stop({ graceSec = 120 } = {}) { return this.call(['stop', '--grace', String(graceSec)], (graceSec + 240) * 1000); }
  /** Back to a snapshot (Fusion), or a fresh clone of the base image (Tart). */
  revert(to) { return this.call(['revert', '--to', to], 2_400_000); }
  snapshot(name) { return this.call(['snapshot', '--name', name], 1_200_000); }
  snapshots() { return this.call(['snapshots'], 120_000).snapshots; }
  clone(base) { return this.call(['clone', '--base', base], 2_400_000); }
  delete() { return this.call(['delete'], 600_000); }
  /** VNC on for the next start; the password is read from the environment variable `passwordEnv` by the broker, never put on a command line. */
  vncOn({ port, passwordEnv }) { return this.call(['vnc', '--port', String(port), '--password-env', passwordEnv], 120_000); }
  vncOff() { return this.call(['vnc', '--off'], 120_000); }
  set(key, value) { return this.call(['set', '--key', key, '--value', value], 120_000); }

  /** Gives the lease back; never throws (a lease that is gone is released). */
  release() {
    try {
      return vm(this.repo, ['release', this.id], { timeoutMs: 60_000, ...this.opts });
    } catch {
      return null;
    }
  }

  /** The fields a result file records. */
  stamp({ snapshot, command } = {}) {
    return { lease: this.id, vm: this.vm, snapshot, command };
  }
}
