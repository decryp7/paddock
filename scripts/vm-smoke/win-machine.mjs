// The per-machine install's host-side steps (Dyno 1.1.0, family ADR 0053): an update announced as the installed version is refused by
// `paddock-setup.exe apply-update` (exit 27, "not newer", nothing left in updates\), and the uninstall removes the programs, the machine-wide
// entries and the start-at-login values. Run as the VM's administrator through the SSH admin key; the install itself (the real UAC prompt) is the suite's.
import { ssh } from './win-helper.mjs';

const PROG = 'C:\\Program Files\\Dyno';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const result = (title, checks, evidence = [], pass = '') => {
  const bad = checks.filter((c) => !c.ok);
  return { title, status: bad.length ? 'fail' : 'pass', summary: bad.length ? bad.map((c) => c.detail).join('; ').slice(0, 240) : pass, checks, evidence };
};

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

/** What the uninstall left, judged from the facts read after it. */
export function judgeUninstall(f) {
  return [
    { ok: !f.folder, detail: f.folder ? `${PROG} is still there after the uninstall (${f.folderFiles})` : `${PROG} is removed` },
    { ok: !f.uninstallKey, detail: f.uninstallKey ? 'the machine-wide uninstall entry is still in the registry' : 'the machine-wide uninstall entry is removed' },
    { ok: !f.service, detail: f.service ? 'the DynoHelper service is still registered' : 'no DynoHelper service is left' },
    { ok: f.processes === 0, detail: f.processes === 0 ? 'no Dyno process is left' : `${f.processes} Dyno process(es) still run` },
    { ok: !f.runValues, detail: f.runValues ? `a start-at-login value for Dyno is left: ${f.runValues}` : 'no start-at-login value for Dyno is left' },
  ];
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
  const has = ssh(`Test-Path '${un}'`, { admin: true }) === 'True';
  checks.push({ ok: has, detail: has ? 'uninstall.exe is in the install folder' : 'no uninstall.exe in the install folder' });
  if (!has) return result('Windows: uninstall removes the programs and the machine-wide entries', checks);
  ssh(`Start-Process '${un}' -ArgumentList '/S'`, { admin: true });
  let facts = {};
  for (let i = 0; i < 40; i++) {
    await sleep(3000);
    facts = {
      folder: ssh(`Test-Path '${PROG}'`, { admin: true }) === 'True',
      folderFiles: ssh(`(Get-ChildItem '${PROG}' -Name -ErrorAction SilentlyContinue) -join ', '`, { admin: true }),
      uninstallKey: ssh("Test-Path 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Dyno'", { admin: true }) === 'True',
      service: ssh('[bool](Get-Service DynoHelper -ErrorAction SilentlyContinue)', { admin: true }) === 'True',
      processes: Number(ssh('(Get-Process dyno-app,dyno-agent,dyno-helper -ErrorAction SilentlyContinue | Measure-Object).Count', { admin: true })) || 0,
      runValues: ssh("(Get-ItemProperty 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run' -ErrorAction SilentlyContinue).PSObject.Properties | Where-Object { $_.Value -like '*Dyno*' } | ForEach-Object { $_.Name }", { admin: true }),
    };
    if (!facts.folder && !facts.uninstallKey) break;
  }
  checks.push(...judgeUninstall(facts));
  return result('Windows: uninstall removes the programs and the machine-wide entries', checks, [JSON.stringify(facts)], 'the programs, the uninstall entry and the start-at-login values are gone');
}

export const afterQuitSteps = [() => sameVersionRefused(), () => uninstall()];
