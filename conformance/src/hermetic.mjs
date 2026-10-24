// C14, dynamic part: `paddock-conformance run -- <command>` runs the tests with HOME, XDG_*, APPDATA and TMPDIR pointing into
// a scratch folder, then checks the aftermath: the scratch temp folder is empty (temp leak), nothing was created in the scratch
// home outside the allow-list, and no process of the run is still alive (process group, plus anything that mentions the scratch path).
import { spawn as nodeSpawn, execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { error } from './finding.mjs';

/** The environment that sends every per-user location into `scratch`. */
export function hermeticEnv(scratch, base = process.env) {
  const home = join(scratch, 'home');
  const tmp = join(scratch, 'tmp');
  return {
    ...base,
    HOME: home,
    USERPROFILE: home,
    XDG_CONFIG_HOME: join(home, '.config'),
    XDG_DATA_HOME: join(home, '.local/share'),
    XDG_CACHE_HOME: join(home, '.cache'),
    XDG_STATE_HOME: join(home, '.local/state'),
    APPDATA: join(home, 'AppData/Roaming'),
    LOCALAPPDATA: join(home, 'AppData/Local'),
    TMPDIR: tmp,
    TMP: tmp,
    TEMP: tmp,
  };
}

const defaultPs = () => {
  try {
    return execFileSync('ps', ['-axo', 'pid=,command='], { encoding: 'utf8', maxBuffer: 16 << 20 });
  } catch {
    return '';
  }
};

const alive = (pid) => {
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    return e.code === 'EPERM';
  }
};

/**
 * Runs `cmd args` hermetically. Returns { status, findings, scratch }. `allowHome`: names the run may create directly in the
 * scratch home. `ps`: injectable process listing for tests.
 */
export async function runHermetic(cmd, args, { allowHome = [], keep = false, ps = defaultPs, base = process.env, cwd = process.cwd(), scratchRoot = tmpdir() } = {}) {
  const scratch = mkdtempSync(join(scratchRoot, 'paddock-hermetic-'));
  for (const d of ['home', 'tmp']) mkdirSync(join(scratch, d));
  const findings = [];
  const posix = process.platform !== 'win32';
  const child = nodeSpawn(cmd, args, { cwd, env: hermeticEnv(scratch, base), stdio: 'inherit', detached: posix });
  const status = await new Promise((resolve) => {
    child.on('error', (e) => {
      findings.push(error('C14', 7, cmd, 1, `cannot start ${cmd}: ${e.message}`, 'Check the command after `run --`.'));
      resolve(127);
    });
    child.on('exit', (code, signal) => resolve(code ?? (signal ? 128 : 1)));
  });

  const left = readdirSync(join(scratch, 'tmp'));
  if (left.length) findings.push(error('C14', 7, scratch, 1, `temp leak: ${left.length} item(s) left in TMPDIR (${left.slice(0, 5).join(', ')})`, 'Create temp files with a guard that removes them (a tempdir that deletes itself), also on failure.'));
  const extra = readdirSync(join(scratch, 'home')).filter((n) => !allowHome.includes(n));
  if (extra.length) findings.push(error('C14', 7, scratch, 1, `the run wrote into the (scratch) home: ${extra.slice(0, 5).join(', ')}`, 'Tests must use their own temp folders, never the user home; add legitimate names to the allow-list.'));

  const survivors = [];
  if (posix && child.pid && alive(-child.pid)) {
    survivors.push(`process group ${child.pid}`);
    try {
      process.kill(-child.pid, 'SIGKILL');
    } catch {
      /* gone meanwhile */
    }
  }
  for (const line of ps().split('\n')) {
    const m = /^\s*(\d+)\s+(.*)$/.exec(line);
    if (m && m[2].includes(scratch) && Number(m[1]) !== process.pid && !m[2].includes('ps -axo')) {
      survivors.push(`pid ${m[1]}`);
      try {
        process.kill(Number(m[1]), 'SIGKILL');
      } catch {
        /* gone meanwhile */
      }
    }
  }
  if (survivors.length) findings.push(error('C14', 7, cmd, 1, `a process started by the run is still alive: ${survivors.join(', ')} (killed)`, 'Stop every process the tests start (drop guards, kill on teardown); no processes left behind.'));

  if (!keep) rmSync(scratch, { recursive: true, force: true });
  return { status, findings, scratch };
}
