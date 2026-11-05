// The per-machine install's host-side steps (Dyno 1.1.0, family ADR 0053): an update announced as the installed version is refused by
// `paddock-setup.exe apply-update` (exit 27, "not newer", nothing left in updates\), and the uninstall removes the programs, the machine-wide
// entries and the start-at-login values. Run as the VM's administrator through the SSH admin key; the install itself (the real UAC prompt) is the suite's.
import { ssh } from './win-helper.mjs';
import { makeResult } from './verdict.mjs';
import { judgeUninstall as judgeUninstallFacts, triState } from './win-judges.mjs';

/** What the uninstall left, judged from tri-state facts (yes, no, unreadable): see win-judges.mjs. */
export const judgeUninstall = (f) => judgeUninstallFacts(f, PROG);

const PROG = 'C:\\Program Files\\Dyno';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const result = makeResult;

/** `apply-update` printed "EXIT=<n>" last. Returns the code, or null. */
export function exitCodeOf(text) {
  const m = /EXIT=(-?\d+)\s*$/.exec(text.trim());
  return m ? Number(m[1]) : null;
}

/** The first three numbers of a Windows ProductVersion ("1.1.0.0" is "1.1.0"). */
export function semverOf(productVersion) {
  const m = /^(\d+)\.(\d+)\.(\d+)/.exec(productVersion.trim());
  return m ? `${m[1]}.${m[2]}.${m[3]}` : null;
}

export function sameVersionRefused() {
  const checks = [];
  const ver = semverOf(ssh(`(Get-Item '${PROG}\\dyno-app.exe').VersionInfo.ProductVersion`));
  checks.push({ ok: Boolean(ver), detail: ver ? `installed version ${ver}` : 'the installed version could not be read' });
  const hasSig = ssh('Test-Path C:\\paddock-test\\vmcheck\\setup.exe.sig') === 'True';
  if (!ver || !hasSig) {
    if (!hasSig) checks.push({ ok: false, detail: 'the installer has no .sig next to it, so the signed update cannot be built' });
    return result('Windows: an update from the same version is refused (not newer)', checks);
  }
  const cmd = `$sig = (Get-Content C:\\paddock-test\\vmcheck\\setup.exe.sig -Raw).Trim(); & '${PROG}\\paddock-setup.exe' apply-update --file C:\\paddock-test\\vmcheck\\setup.exe --signature $sig --version ${ver} --restart 2>&1 | Out-String; "EXIT=$LASTEXITCODE"`;
  const out = ssh(cmd, { admin: true, ms: 120_000 });
  const code = exitCodeOf(out);
  checks.push({ ok: code === 27, detail: code === 27 ? 'apply-update exit 27 (not newer than the installed version)' : `apply-update exit ${code} (expected 27 "not newer")` });
  const left = ssh(`Get-ChildItem '${PROG}\\updates' -Name -ErrorAction SilentlyContinue | Where-Object { $_ -ne 'setup.log' }`, { admin: true }).split('\n').map((l) => l.trim()).filter(Boolean);
  checks.push({ ok: left.length === 0, detail: left.length ? `a refused update left ${left.join(', ')} in updates\\` : 'a refused update leaves nothing in updates\\' });
  const tail = ssh(`Get-Content '${PROG}\\updates\\setup.log' -Tail 8 -ErrorAction SilentlyContinue`, { admin: true }).split('\n');
  return result('Windows: an update from the same version is refused (not newer)', checks, [...out.split('\n').slice(0, 10), '--- setup.log tail', ...tail], 'apply-update refused the same version (exit 27) and left nothing');
}

export async function uninstall() {
  const checks = [];
  ssh('Get-Process dyno-app,dyno-agent,dyno-bench -ErrorAction SilentlyContinue | Stop-Process -Force', { admin: true });
  const un = `${PROG}\\uninstall.exe`;
  const hasState = triState(ssh(`Test-Path '${un}'`, { admin: true }));
  const has = hasState === 'yes';
  checks.push({ ok: has, ...(hasState === 'unreadable' ? { kind: 'check' } : {}), detail: has ? 'uninstall.exe is in the install folder' : hasState === 'unreadable' ? 'the install folder could not be read (SSH printed nothing)' : 'no uninstall.exe in the install folder' });
  if (!has) return result('Windows: uninstall removes the programs and the machine-wide entries', checks);
  ssh(`Start-Process '${un}' -ArgumentList '/S'`, { admin: true });
  let facts = {};
  // Three-state reads (yes, no, unreadable): a dead SSH prints nothing and must not read as "removed". The start-at-login value of the standard
  // user is read over the user's own SSH (that is paddock-test's HKCU, not the administrator's).
  const adminBool = (cmd) => triState(ssh(cmd, { admin: true }));
  for (let i = 0; i < 40; i++) {
    await sleep(3000);
    const count = ssh('(Get-Process dyno-app,dyno-agent,dyno-helper,dyno-bench -ErrorAction SilentlyContinue | Measure-Object).Count', { admin: true });
    const runMachine = ssh("(Get-ItemProperty 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run' -ErrorAction SilentlyContinue).PSObject.Properties | Where-Object { $_.Value -like '*Dyno*' } | ForEach-Object { $_.Name }", { admin: true });
    const runUser = ssh("(Get-ItemProperty 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run' -ErrorAction SilentlyContinue).PSObject.Properties | Where-Object { $_.Value -like '*Dyno*' } | ForEach-Object { $_.Name }; 'END'");
    facts = {
      folder: adminBool(`Test-Path '${PROG}'`),
      folderFiles: ssh(`(Get-ChildItem '${PROG}' -Name -ErrorAction SilentlyContinue) -join ', '`, { admin: true }),
      uninstallKey: adminBool("Test-Path 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Dyno'"),
      service: adminBool('[bool](Get-Service DynoHelper -ErrorAction SilentlyContinue)'),
      processes: /^\d+$/.test(count.trim()) ? Number(count) : 'unreadable',
      // Machine-wide Run: an empty answer from a live SSH means none; the end marker of the user read tells a dead SSH from "no values".
      runMachine: adminBool("Test-Path 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run'") === 'unreadable' ? 'unreadable' : runMachine.trim() ? 'yes' : 'no',
      runMachineNames: runMachine.trim(),
      runUser: /END\s*$/.test(runUser) ? (runUser.replace(/END\s*$/, '').trim() ? 'yes' : 'no') : 'unreadable',
      runUserNames: runUser.replace(/END\s*$/, '').trim(),
    };
    if (facts.folder === 'no' && facts.uninstallKey === 'no') break;
  }
  checks.push(...judgeUninstall(facts));
  return result('Windows: uninstall removes the programs and the machine-wide entries', checks, [JSON.stringify(facts)], 'the programs, the uninstall entry and the start-at-login values are gone');
}

export const afterQuitSteps = [() => sameVersionRefused(), () => uninstall()];
