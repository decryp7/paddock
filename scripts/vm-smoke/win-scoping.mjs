// Security D6 on Windows: what a second standard account sees through the helper. A second account is created in the guest (reverted with the VM),
// Dyno's own files are copied where it can run them, its agent is started as that account in a scheduled task, and `dyno top --json` and
// `dyno snapshot` (its view of processes and, where the snapshot carries it, traffic) are compared with the owner's view. A uniquely named process of
// the owner (owner-secret-probe.exe) must show for the owner and must not appear anywhere in the second account's views, which must carry an
// "other users" total instead. Needs the helper ON (helperSteps runs it between Turn on and Turn off) and the host's admin SSH key.
import { randomBytes } from 'node:crypto';
import { ssh } from './win-helper.mjs';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const PUB = 'C:\\Users\\Public';
const result = (title, checks, evidence = [], pass = '') => { const bad = checks.filter((c) => !c.ok); return { title, status: bad.length ? 'fail' : 'pass', summary: bad.length ? bad.map((c) => c.detail).join('; ').slice(0, 240) : pass, checks, evidence }; };

export async function scopingStep() {
  const checks = [];
  const evidence = [];
  const pw = randomBytes(12).toString('hex').replace(/[a-f]*$/, '') + 'zq' + randomBytes(4).toString('hex'); // lowercase and digits, never stored
  const dyno = 'C:\\Program Files\\Dyno'; // the per-machine install (family ADR 0053)
  // The owner: a probe process with a name nobody else has, and the owner's own view (the positive control).
  // A busy probe (a copy of powershell.exe under a unique name) so it ranks in `dyno top` by CPU.
  ssh(`Copy-Item "$env:SystemRoot\\System32\\WindowsPowerShell\\v1.0\\powershell.exe" "C:\\Users\\paddock-test\\owner-secret-probe.exe" -Force; Start-Process "C:\\Users\\paddock-test\\owner-secret-probe.exe" -ArgumentList "-NoProfile","-Command","while($true){ }" -WindowStyle Hidden`);
  await sleep(4000);
  const ownerView = ssh(`& "${dyno}\\dyno.exe" top --by cpu -n 400 --json 2>&1 | Out-String`);
  checks.push({ ok: /owner-secret-probe/i.test(ownerView), kind: /owner-secret-probe/i.test(ownerView) ? undefined : 'check', detail: /owner-secret-probe/i.test(ownerView) ? 'positive control: the owner sees its own probe process' : 'positive control failed: the owner does not see its own probe process in `dyno top`' });
  // The second account.
  ssh('$p = [Console]::In.ReadLine(); net user paddock-user2 $p /add /y | Out-Null; net localgroup Users paddock-user2 /add 2>$null | Out-Null', { admin: true, input: `${pw}\n` });
  ssh(`Remove-Item "${PUB}\\dyno2" -Recurse -Force -ErrorAction SilentlyContinue; Copy-Item "${dyno}" "${PUB}\\dyno2" -Recurse -Force; Remove-Item "${PUB}\\dyno2\\*.json","${PUB}\\dyno2\\*.db*","${PUB}\\dyno2\\agent.lock" -Force -ErrorAction SilentlyContinue; icacls "${PUB}\\dyno2" /grant "BUILTIN\\Users:(OI)(CI)(RX)" | Out-Null`, { admin: true });
  const script = `$ErrorActionPreference='Continue'; $d='${PUB}\\dyno2'; $env:DYNO_DATA_DIR='${PUB}\\dyno2-data'; New-Item -ItemType Directory -Force $env:DYNO_DATA_DIR | Out-Null; Start-Process "$d\\dyno-agent.exe" -ArgumentList 'run' -WindowStyle Hidden; Start-Sleep 25; whoami | Out-File ${PUB}\\dyno2-whoami.txt; & "$d\\dyno.exe" top --by cpu -n 400 --json 2>&1 | Out-File ${PUB}\\dyno2-top.json; & "$d\\dyno.exe" snapshot 2>&1 | Out-File ${PUB}\\dyno2-snapshot.json; Get-Process dyno-agent -ErrorAction SilentlyContinue | Where-Object { $_.SessionId -ge 0 } | Stop-Process -Force -ErrorAction SilentlyContinue; 'done' | Out-File ${PUB}\\dyno2-done.txt`;
  ssh(`Set-Content "${PUB}\\dyno2-run.ps1" '${script.replace(/'/g, "''")}'; Remove-Item ${PUB}\\dyno2-done.txt -ErrorAction SilentlyContinue; icacls "${PUB}\\dyno2-run.ps1" /grant "BUILTIN\\Users:(RX)" | Out-Null`, { admin: true });
  ssh(`schtasks /create /tn Dyno2 /sc once /st 00:00 /ru paddock-user2 /rp ${pw} /tr "cmd /c powershell -NoProfile -ExecutionPolicy Bypass -File ${PUB}\\dyno2-run.ps1 > ${PUB}\\dyno2-run.log 2>&1" /f | Out-Null; schtasks /run /tn Dyno2 | Out-Null`, { admin: true });
  let done = false;
  for (let i = 0; i < 40 && !done; i++) { await sleep(4000); done = ssh(`Test-Path ${PUB}\\dyno2-done.txt`, { admin: true }) === 'True'; }
  checks.push({ ok: done, detail: done ? 'the second account ran its own Dyno agent and read its view' : 'the second account\'s run did not finish within 160 s (see dyno2-run.log)' });
  const diagRun = ssh(`Get-Content ${PUB}\\dyno2-run.log -ErrorAction SilentlyContinue | Select-Object -First 8; schtasks /query /tn Dyno2 /v /fo list | Select-String 'Last Result|Status|Run As User'`, { admin: true });
  evidence.push('--- second account task', ...diagRun.split('\n'), `--- owner's top (first 300 chars): ${ownerView.slice(0, 300).replace(/\s+/g, ' ')}`);
  const who = ssh(`Get-Content ${PUB}\\dyno2-whoami.txt -ErrorAction SilentlyContinue`, { admin: true });
  const top = ssh(`Get-Content ${PUB}\\dyno2-top.json -Raw -ErrorAction SilentlyContinue`, { admin: true });
  const snap = ssh(`Get-Content ${PUB}\\dyno2-snapshot.json -Raw -ErrorAction SilentlyContinue`, { admin: true });
  evidence.push(`second account: ${who.trim() || '(whoami unreadable)'}`, `its top view: ${top.length} chars; snapshot: ${snap.length} chars`);
  checks.push({ ok: /paddock-user2/i.test(who), detail: `the run was the second standard account (${who.trim() || 'unknown'})` });
  const both = `${top}\n${snap}`;
  checks.push({ ok: top.length > 50 && !/owner-secret-probe/i.test(both), detail: /owner-secret-probe/i.test(both) ? 'D6 VIOLATION: the second account sees the owner\'s process name owner-secret-probe in its views' : 'the second account does not see the owner\'s process name' });
  checks.push({ ok: !/paddock-test/i.test(both.replace(/C:\\Users\\Public[^"\s]*/gi, '')), detail: /paddock-test/i.test(both) ? 'the second account\'s views name the owner account paddock-test' : 'the second account\'s views name no other account' });
  const other = /other users|Other users|othersTotal|others/.test(both);
  checks.push({ ok: other, detail: other ? 'the views carry an "other users" total' : 'no "other users" total in the second account\'s views (processes of other accounts are not summarised)' });
  evidence.push(`traffic in the snapshot: ${/traffic|bytes/i.test(snap) ? 'present' : 'not carried by `dyno snapshot`; Network > By program as a second account needs its own window'}`);
  // Clean up what this step made.
  ssh(`schtasks /delete /tn Dyno2 /f 2>$null | Out-Null; Get-Process owner-secret-probe -ErrorAction SilentlyContinue | Stop-Process -Force; Remove-Item "C:\\Users\\paddock-test\\owner-secret-probe.exe","${PUB}\\dyno2*" -Recurse -Force -ErrorAction SilentlyContinue; net user paddock-user2 /delete 2>$null | Out-Null`, { admin: true });
  return result('Windows: D6 scoping, a second standard account sees only its own and the system\'s programs', checks, evidence, 'the second account sees no other account\'s programs and gets an "other users" total');
}
