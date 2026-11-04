// The host side of win-act.ps1: sends one input or window command to the in-session actor of the Windows test VM and returns its answer.
// Commands are written as files through the standard user's SSH; every wait is bounded.
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const VMWIN = process.env.PADDOCK_VM_WIN ?? fileURLToPath(new URL('../vm-win.sh', import.meta.url));
const DIR = 'C:\\paddock-test\\vmcheck\\act';
const run = (cmd, timeoutMs = 60_000) => spawnSync('bash', [VMWIN, 'ssh', cmd], { encoding: 'utf8', timeout: timeoutMs }).stdout.split('\n').filter((l) => !l.startsWith('**')).join('\n').trim();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export class Actor {
  constructor() { this.n = 0; }

  /** Starts the in-session loop (a scheduled task in the console session) after copying nothing: the script is staged by the caller. */
  start(maxMin = 40) {
    // `cmd /c` with redirected output is the form that starts reliably in the console session (a bare hidden powershell task did not).
    run(`schtasks /end /tn VmAct2 2>$null | Out-Null; Remove-Item ${DIR} -Recurse -Force -ErrorAction SilentlyContinue; schtasks /create /tn VmAct2 /sc once /st 00:00 /ru paddock-test /it /f /tr "cmd /c powershell -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File C:\\paddock-test\\vmcheck\\win-act.ps1 -Dir ${DIR} -MaxMin ${maxMin} > C:\\paddock-test\\vmcheck\\act-run.err 2>&1" | Out-Null; schtasks /run /tn VmAct2 | Out-Null`);
  }

  async send(line, guardMs = 60_000) {
    const n = ++this.n;
    run(`Set-Content -Path ${DIR}\\act-${n}.txt -Value '${line.replace(/'/g, "''")}'`);
    const end = Date.now() + guardMs;
    while (Date.now() < end) {
      const r = run(`if (Test-Path ${DIR}\\act-${n}.resp) { Get-Content ${DIR}\\act-${n}.resp -Raw } else { '__none__' }`);
      if (r !== '__none__') return r;
      await sleep(700);
    }
    throw new Error(`the actor did not answer "${line}" within ${guardMs / 1000} s`);
  }
}
