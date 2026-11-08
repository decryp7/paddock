// Security D6 on Windows: what a second standard account sees through the helper. A second account is created in the guest (reverted with the VM),
// Dyno's own files are copied where it can run them, its agent is started as that account in a scheduled task, and `dyno top --json` and
// `dyno snapshot` (its view of processes and, where the snapshot carries it, traffic) are compared with the owner's view. A uniquely named process of
// the owner (owner-secret-probe.exe) must show for the owner and must not appear anywhere in the second account's views, which must carry an
// "other users" total instead. Needs the helper ON (helperSteps runs it between Turn on and Turn off) and the host's admin SSH key.
import { randomBytes } from 'node:crypto';
import { ssh } from './win-helper.mjs';
import { makeResult } from './verdict.mjs';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const PUB = 'C:\\Users\\Public';
// A broken measurement (the task never ran, the positive control unseen) is check-broken, never a product FAIL (ER-33).
const result = makeResult;

export async function scopingStep() {
  const checks = [];
  const evidence = [];
  const pw = randomBytes(12).toString('hex').replace(/[a-f]*$/, '') + 'zq' + randomBytes(4).toString('hex'); // lowercase and digits, never stored
  const dyno = 'C:\\Program Files\\Dyno'; // the per-machine install (family ADR 0053)
  // The owner: a probe process with a name nobody else has, and the owner's own view (the positive control).
  // A busy probe (a copy of powershell.exe under a unique name) so it ranks in `dyno top` by CPU.
  // `dyno top` groups processes by the program's own name (FileDescription first): a renamed powershell.exe shows as "Windows PowerShell" (7d06 and 0ad rounds), so the
  // probe is a tiny program built here whose title, product and file name are all owner-secret-probe.
  const probeSrc = 'using System.Reflection; [assembly: AssemblyTitle("owner-secret-probe")] [assembly: AssemblyProduct("owner-secret-probe")] [assembly: AssemblyDescription("owner-secret-probe")] public static class P { public static void Main() { while (true) { } } }';
  const built = ssh(`Add-Type -TypeDefinition '${probeSrc}' -OutputAssembly "C:\\Users\\paddock-test\\owner-secret-probe.exe" -OutputType ConsoleApplication -ErrorAction Stop 2>&1 | Out-String; (Get-Item "C:\\Users\\paddock-test\\owner-secret-probe.exe").VersionInfo.FileDescription; Start-Process "C:\\Users\\paddock-test\\owner-secret-probe.exe" -WindowStyle Hidden; Start-Sleep 2; (Get-Process owner-secret-probe -ErrorAction SilentlyContinue | Measure-Object).Count`);
  evidence.push(`probe built and started (FileDescription, running count): ${built.trim().replace(/\s+/g, ' ').slice(0, 200)}`);
  await sleep(4000);
  // Polled (the sampler lists a new process after a tick or two), up to 30 s; the whole view is kept as evidence.
  let ownerView = '';
  for (let i = 0; i < 7 && !/owner-secret-probe/i.test(ownerView); i++) { if (i) await sleep(5000); ownerView = ssh(`& "${dyno}\\dyno.exe" top --by cpu -n 100 --json 2>&1 | Out-String`); }
  evidence.push(`--- owner's top view (full): ${ownerView.replace(/\s+/g, ' ').slice(0, 2500)}`);
  checks.push({ ok: /owner-secret-probe/i.test(ownerView), kind: /owner-secret-probe/i.test(ownerView) ? undefined : 'check', detail: /owner-secret-probe/i.test(ownerView) ? 'positive control: the owner sees its own probe process' : 'positive control failed: the owner does not see its own probe process in `dyno top`' });
  // The second account.
  ssh('$p = [Console]::In.ReadLine(); net user paddock-user2 $p /add /y | Out-Null; net localgroup Users paddock-user2 /add 2>$null | Out-Null', { admin: true, input: `${pw}\n` });
  // A scheduled task with a stored password needs "Log on as a batch job" (SeBatchLogonRight), which a plain Users member lacks (7d06: SCHED_S_TASK_HAS_NOT_RUN).
  // Granted to this account only, in the reverted guest (secedit, USER_RIGHTS area); nothing else changes.
  const grant = ssh(`$sid = (New-Object Security.Principal.NTAccount('paddock-user2')).Translate([Security.Principal.SecurityIdentifier]).Value; $f = "$env:TEMP\\ur.inf"; $db = "$env:TEMP\\ur.sdb"; secedit /export /cfg $f /areas USER_RIGHTS | Out-Null; $c = Get-Content $f; if ($c -match '^SeBatchLogonRight') { $c = $c -replace '^(SeBatchLogonRight\\s*=.*)$', ('$1,*' + $sid) } else { $c = $c -replace '^\\[Privilege Rights\\]$', ("[Privilege Rights]\`r\`nSeBatchLogonRight = *" + $sid) }; $c | Set-Content $f -Encoding Unicode; secedit /configure /db $db /cfg $f /areas USER_RIGHTS | Out-Null; "EXIT=$LASTEXITCODE"; (Get-Content $f | Select-String SeBatchLogonRight).Line`, { admin: true });
  evidence.push(`batch logon right for paddock-user2: ${grant.trim().replace(/\s+/g, ' ').slice(0, 200)}`);
  ssh(`Remove-Item "${PUB}\\dyno2" -Recurse -Force -ErrorAction SilentlyContinue; Copy-Item "${dyno}" "${PUB}\\dyno2" -Recurse -Force; Remove-Item "${PUB}\\dyno2\\*.json","${PUB}\\dyno2\\*.db*","${PUB}\\dyno2\\agent.lock" -Force -ErrorAction SilentlyContinue; icacls "${PUB}\\dyno2" /grant "BUILTIN\\Users:(OI)(CI)(RX)" | Out-Null`, { admin: true });
  const script = `$ErrorActionPreference='Continue'; $d='${PUB}\\dyno2'; $env:DYNO_DATA_DIR='${PUB}\\dyno2-data'; New-Item -ItemType Directory -Force $env:DYNO_DATA_DIR | Out-Null; Start-Process "$d\\dyno-agent.exe" -ArgumentList 'run' -WindowStyle Hidden; Start-Sleep 25; whoami | Out-File ${PUB}\\dyno2-whoami.txt; & "$d\\dyno.exe" top --by cpu -n 100 --json 2>&1 | Out-File ${PUB}\\dyno2-top.json; & "$d\\dyno.exe" snapshot 2>&1 | Out-File ${PUB}\\dyno2-snapshot.json; Get-Process dyno-agent -ErrorAction SilentlyContinue | Where-Object { $_.SessionId -ge 0 } | Stop-Process -Force -ErrorAction SilentlyContinue; 'done' | Out-File ${PUB}\\dyno2-done.txt`;
  ssh(`Set-Content "${PUB}\\dyno2-run.ps1" '${script.replace(/'/g, "''")}'; Remove-Item ${PUB}\\dyno2-done.txt -ErrorAction SilentlyContinue; icacls "${PUB}\\dyno2-run.ps1" /grant "BUILTIN\\Users:(RX)" | Out-Null`, { admin: true });
  ssh(`schtasks /create /tn Dyno2 /sc once /st 00:00 /ru paddock-user2 /rp ${pw} /tr "cmd /c powershell -NoProfile -ExecutionPolicy Bypass -File ${PUB}\\dyno2-run.ps1 > ${PUB}\\dyno2-run.log 2>&1" /f | Out-Null; schtasks /run /tn Dyno2 | Out-Null`, { admin: true });
  let done = false;
  for (let i = 0; i < 40 && !done; i++) { await sleep(4000); done = ssh(`Test-Path ${PUB}\\dyno2-done.txt`, { admin: true }) === 'True'; }
  checks.push({ ok: done, ...(done ? {} : { kind: 'check' }), detail: done ? 'the second account ran its own Dyno agent and read its view' : 'the second account\'s task did not finish within 160 s (it may never have started; see the task\'s Last Result): the measurement, not the product' });
  const diagRun = ssh(`Get-Content ${PUB}\\dyno2-run.log -ErrorAction SilentlyContinue | Select-Object -First 8; schtasks /query /tn Dyno2 /v /fo list | Select-String 'Last Result|Status|Run As User'`, { admin: true });
  evidence.push('--- second account task', ...diagRun.split('\n'), `--- owner's top (first 300 chars): ${ownerView.slice(0, 300).replace(/\s+/g, ' ')}`);
  if (!done) {
    ssh(`schtasks /delete /tn Dyno2 /f 2>$null | Out-Null; Get-Process owner-secret-probe -ErrorAction SilentlyContinue | Stop-Process -Force; Remove-Item "C:\\Users\\paddock-test\\owner-secret-probe.exe","${PUB}\\dyno2*" -Recurse -Force -ErrorAction SilentlyContinue; net user paddock-user2 /delete 2>$null | Out-Null`, { admin: true });
    return result('Windows: D6 scoping, a second standard account sees only its own and the system\'s programs', checks, evidence, '');
  }
  const who = ssh(`Get-Content ${PUB}\\dyno2-whoami.txt -ErrorAction SilentlyContinue`, { admin: true });
  const top = ssh(`Get-Content ${PUB}\\dyno2-top.json -Raw -ErrorAction SilentlyContinue`, { admin: true });
  const snap = ssh(`Get-Content ${PUB}\\dyno2-snapshot.json -Raw -ErrorAction SilentlyContinue`, { admin: true });
  evidence.push(`second account: ${who.trim() || '(whoami unreadable)'}`, `its top view: ${top.length} chars; snapshot: ${snap.length} chars`, `--- its top view (full): ${top.replace(/\s+/g, ' ').slice(0, 2000)}`);
  checks.push({ ok: /paddock-user2/i.test(who), detail: `the run was the second standard account (${who.trim() || 'unknown'})` });
  const both = `${top}\n${snap}`;
  checks.push({ ok: top.length > 50 && !/owner-secret-probe/i.test(both), detail: /owner-secret-probe/i.test(both) ? 'D6 VIOLATION: the second account sees the owner\'s process name owner-secret-probe in its views' : 'the second account does not see the owner\'s process name' });
  checks.push({ ok: !/paddock-test/i.test(both.replace(/C:\\Users\\Public[^"\s]*/gi, '')), detail: /paddock-test/i.test(both) ? 'the second account\'s views name the owner account paddock-test' : 'the second account\'s views name no other account' });
  const other = /other users|Other users|othersTotal|others/.test(both);
  checks.push({ ok: other, detail: other ? 'the views carry an "other users" total' : 'no "other users" total in the second account\'s views (processes of other accounts are not summarised)' });
  evidence.push(`traffic in the snapshot: ${/traffic|bytes/i.test(snap) ? 'present' : 'not carried by `dyno snapshot`; Network > By program as a second account needs its own window'}`);
  // Clean up what this step made.
  ssh(`schtasks /delete /tn Dyno2 /f 2>$null | Out-Null; Get-Process owner-secret-probe -ErrorAction SilentlyContinue | Stop-Process -Force; Remove-Item "C:\\Users\\paddock-test\\owner-secret-probe.exe","${PUB}\\dyno2*" -Recurse -Force -ErrorAction SilentlyContinue; net user paddock-user2 /delete 2>$null | Out-Null`, { admin: true });
  // Without the positive control (the owner sees its own probe) nothing the second account does not see proves anything: those checks are the measurement's.
  const control = checks[0]?.ok === true;
  const judged = control ? checks : checks.map((c, i) => (i === 0 || c.ok ? c : { ...c, kind: 'check', detail: `${c.detail} (not conclusive: the positive control failed)` }));
  return result('Windows: D6 scoping, a second standard account sees only its own and the system\'s programs', judged, evidence, 'the second account sees no other account\'s programs and gets an "other users" total');
}
