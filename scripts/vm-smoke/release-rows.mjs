// The release rows collected for Dyno 1.1.0 beside the start, helper and rule 71 rows (manifest.mjs requires them). A row that can be driven has a pure judge here;
// a row that cannot be driven yet is a step with status `not-measured` and the reason, never a pass (ER-33), so the run is INCOMPLETE until it is.
import { makeResult } from './verdict.mjs';

const chk = (ok, detail, kind) => ({ ok, detail, ...(kind ? { kind } : {}) });

/** A row nobody drives yet: it is in the manifest, so the verdict lists it as not measured with this reason. */
export const notDriven = (title, reason) => ({ title, status: 'not-measured', cls: 'check', summary: `not measured: ${reason}`, checks: [{ ok: false, kind: 'check', detail: reason }], evidence: [] });

/** macOS: a standard user must be able to read the helper lifecycle log (`sudo -u nobody head` of the file; nobody is a standard account). */
export function judgeLifecycleLog({ code, out }) {
  if (code === null) return [chk(false, 'the log could not be read at all (no exit code)', 'check')];
  const lines = String(out ?? '').split('\n').filter((l) => l.trim());
  return [
    chk(code === 0, code === 0 ? 'a standard user (nobody) reads /Library/Application Support/Dyno/helper-lifecycle.log' : `a standard user cannot read helper-lifecycle.log (exit ${code}): ${lines[0] ?? ''}`),
    chk(lines.length > 0, lines.length ? `the log has content (${lines.length} line(s) read)` : 'the log is empty: nothing in it explains a helper lifecycle'),
  ];
}

/** Linux: after the package is removed the helper's log folder must still list its log (`ls /var/log/dyno-helper`). */
export function judgeLinuxHelperLogDir({ code, out }) {
  if (code === null) return [chk(false, 'ls /var/log/dyno-helper could not be run', 'check')];
  const names = String(out ?? '').split('\n').map((l) => l.trim()).filter(Boolean);
  return [chk(code === 0 && names.length > 0, code === 0 && names.length ? `/var/log/dyno-helper still lists ${names.join(', ')} after the uninstall` : `/var/log/dyno-helper is ${code === 0 ? 'empty' : 'gone'} after the uninstall (ls exit ${code})`)];
}

/**
 * macOS Quit and stop measuring, then reopen: the agent is back; whether "Background Items Added" appeared is RECORDED (seen or not, as information); an item the
 * user switched off in Login Items stays off.
 * @param {{agentBack:boolean, notificationSeen:boolean|null, loginItemWasOff:boolean|null, loginItemNowOff:boolean|null}} f
 */
export function judgeMacReopen(f) {
  const checks = [chk(f.agentBack, f.agentBack ? 'reopening Dyno brought the agent back' : 'reopening Dyno did not bring the agent back')];
  checks.push({ ok: true, info: true, detail: f.notificationSeen === null ? '"Background Items Added" could not be looked for' : `"Background Items Added" ${f.notificationSeen ? 'WAS' : 'was not'} seen after the reopen` });
  if (f.loginItemWasOff === null || f.loginItemNowOff === null) checks.push(chk(false, 'the Login Items state could not be read (sfltool dumpbtm)', 'check'));
  else if (!f.loginItemWasOff) checks.push(chk(false, 'the Login Items entry was not switched off before the test, so "stays off" was not tested', 'check'));
  else checks.push(chk(f.loginItemNowOff, f.loginItemNowOff ? 'the Login Items entry the user switched off stays off after the reopen' : 'Dyno switched its Login Items entry back on after the user switched it off'));
  return checks;
}

/**
 * MEASUREMENT, not a release row: a key the app made itself in build A (so the item's partition names A's signing identity), then build B installed over it:
 * does B show a window, and how soon? Today B blocks on the keychain before any window (measured 2026-11-05: no window within 45 s, the main thread in
 * SecKeychainFindGenericPassword, a "Dyno wants to use your confidential information" sheet). That is a PRODUCT bug whose fix is coming separately.
 * The step is therefore always `not-measured` with the numbers in `summary` and in info checks: it is never `pass` (nothing may read a measured delay as a
 * verdict) and never `fail` (a fail would turn every run red before the fix lands); it is NOT in manifest.mjs, so the verdict ignores it. When the product
 * fix lands, its owner turns this into a judged row (pass = a window within the limit they name) and adds it to the manifest.
 * `p`: { windowSeen: boolean|null (null: the probe could not look), windowMs: number|null, sheetSeen: boolean|null, guardMs }.
 */
export function judgeKeychainOtherBuild(p) {
  const title = ROW_TITLES.keychainOtherBuild;
  if (p.windowSeen === null || p.windowSeen === undefined) return { title, status: 'not-measured', cls: 'check', summary: 'not measured: the window probe could not look (check broken)', checks: [{ ok: false, kind: 'check', detail: 'the window probe returned no answer' }], evidence: [] };
  const said = p.windowSeen ? `a window appeared after ${(p.windowMs / 1000).toFixed(1)} s` : `no window within the ${(p.guardMs / 1000).toFixed(0)} s guard`;
  const sheet = p.sheetSeen === null || p.sheetSeen === undefined ? 'the keychain sheet was not looked for' : `the keychain sheet ${p.sheetSeen ? 'WAS' : 'was not'} on screen`;
  return {
    title, status: 'not-measured', cls: 'check', summary: `measured, not judged until the product fix lands: build B over a key made by build A: ${said}; ${sheet}`,
    checks: [{ ok: true, info: true, detail: `window: ${said}` }, { ok: true, info: true, detail: sheet }], evidence: [],
  };
}

/**
 * Drives the measurement through guest primitives (the caller installs the builds): `createKeyInBuildA()`, `reopenSameBuild()` (quit, open build A again),
 * `installBuildB()`, `openApp()`, `windowVisible()` (boolean), `sheetVisible()` (boolean), `now()`, `sleep(ms)`.
 * The POSITIVE CONTROL comes first (ER-33): build A, which made the key, is reopened and must show a window; if it does not, the probe or the key is broken and the
 * step is "check broken", never a statement about build B. Any primitive that throws is also a broken check.
 */
export async function keychainOtherBuildStep(p, guardMs = 45_000) {
  const broken = (why) => ({ title: ROW_TITLES.keychainOtherBuild, status: 'not-measured', cls: 'check', summary: `not measured: ${String(why).slice(0, 180)} (check broken)`, checks: [{ ok: false, kind: 'check', detail: String(why).slice(0, 220) }], evidence: [] });
  const waitWindow = async () => {
    const t0 = p.now();
    while (p.now() - t0 < guardMs) { if (await p.windowVisible()) return p.now() - t0; await p.sleep(1000); }
    return null;
  };
  try {
    await p.createKeyInBuildA();
    await p.reopenSameBuild();
    const control = await waitWindow();
    if (control === null) return broken(`the positive control failed: build A did not show a window within ${guardMs / 1000} s with its own key, so the probe or the key cannot be trusted`);
    await p.installBuildB();
    await p.openApp();
    const windowMs = await waitWindow();
    const r = judgeKeychainOtherBuild({ windowSeen: windowMs !== null, windowMs, sheetSeen: await p.sheetVisible(), guardMs });
    return { ...r, summary: `${r.summary}; positive control: build A's own key gave a window after ${(control / 1000).toFixed(1)} s`, checks: [{ ok: true, info: true, detail: `positive control: build A reopened with its own key, window after ${(control / 1000).toFixed(1)} s` }, ...r.checks] };
  } catch (e) {
    return broken(e.message ?? e);
  }
}

/** The step of a driven judge. */
export const rowStep = (title, checks, pass) => makeResult(title, checks, [], pass);

export const ROW_TITLES = {
  winReinstall: 'Release row: reinstall over a leftover uninstall.exe works',
  winFirstUninstall: 'Release row: the first uninstall after boot notes the leftover',
  macReopen: 'Release row: macOS Quit and stop measuring, then reopen',
  macLifecycleLog: 'Release row: a standard user reads helper-lifecycle.log',
  linuxLogDir: 'Release row: Linux uninstall leaves /var/log/dyno-helper',
  hungPage: 'Release row: a hung or paused visible page gets one question, no stack',
  secondUser: 'Release row: a second user\'s session sees "An update is being installed"',
  killInstaller: 'Release row: killing the installer on the fixed build gives "Not installed"',
  installAgain: 'Release row: install again works',
  keychainOtherBuild: 'Measurement (not a release row, not in the manifest): a key made by build A, build B installed over it, does B show a window',
};

/** The reasons rows cannot be driven yet (kept in one place so the hand-off and the results say the same). */
export const NOT_DRIVEN = {
  winReinstall: 'needs a second install with its own UAC prompt answered over VNC after the uninstall left a uninstall.exe; the suite answers only the first prompt',
  winFirstUninstall: 'needs a reboot between the leftover and the uninstall plus a read of the note the uninstaller writes; the note\'s file is not named in the product yet',
  macReopen: 'needs the "Quit and stop measuring" menu item clicked over AX and the Login Items state read with sfltool; neither is wired in the macOS driver yet',
  hungPage: 'needs a way to hang or pause a visible page (a debugger pause over CDP on Windows; no equivalent wired for AX or AT-SPI)',
  secondUser: 'needs a second console logon (a fast-user-switch) that the VM driver cannot make',
  killInstaller: 'needs the installer killed mid-run and the app\'s "Not installed" text read; the kill point and the text are not specified',
  installAgain: 'needs a second install with its own UAC prompt answered over VNC (the suite answers only the first)',
};

/** `sfltool dumpbtm` text to whether Dyno's login item is enabled: true, false, or null when no Dyno entry (or no disposition) is in the text. */
export function btmEnabled(text) {
  const blocks = String(text ?? '').split(/\n\s*\n|(?=\n\s*#\d+:)/);
  for (const b of blocks) {
    if (!/net\.decryptology\.dyno|Dyno/i.test(b)) continue;
    const m = /Disposition:\s*\[([^\]]*)\]/.exec(b);
    if (m) return /\benabled\b/.test(m[1]);
  }
  return null;
}

/**
 * Measurements for the macOS round of refactor/dyno-service-shared (prepared, NOT in the manifest: each answers a question the branch left open, and a measurement is recorded,
 * not judged, until its owner names the expected value). `how` is the guest method; `record` what the result file must contain.
 */
export const MAC_SERVICE_MEASUREMENTS = [
  { id: 'toggle-on-timing', question: 'How long does "Start at login" take to switch on with the app\'s own agent?', how: 'press the switch over AX, poll `launchctl print gui/<uid>/net.decryptology.dyno.agent` every 250 ms', record: 'ms from the press to state = running, and to the card reading On' },
  { id: 'banner-count-update', question: 'How many "Background Items Added" banners follow a real 1.0.0 to 1.1.0 update, and a light resume?', how: 'install 1.0.0, register, update to 1.1.0 by the in-app updater, `log show` for the notice since the update; repeat after a lid-close style resume (`pmset sleepnow` is not available in a VM: use a forced `launchctl kickstart`)', record: 'count per case and the log lines' },
  { id: 'downgrade-spawn-78', question: 'Does a real downgrade give `spawn failed` with exit 78?', how: 'install 1.1.0, register, replace the bundle with 1.0.0, bootstrap the job, read `launchctl print` last exit code and `log show`', record: 'last exit code, whether 78 appears, the log lines' },
  { id: 'status-from-dmg', question: 'What does SMAppService.status report from a DMG or translocated copy while the installed job runs?', how: 'open the DMG copy and a quarantined copy, call the app\'s status command (or the helper tool `smappservice status`) while the installed job runs', record: 'status per copy' },
  { id: 'status-lag-register', question: 'How long is the status stale right after `register`?', how: 'register, poll status every 100 ms until it changes', record: 'ms until the first changed status, and the sequence of statuses' },
  { id: 'unregister-never-registered', question: 'What does `unregister` do on a never-registered item?', how: 'call unregister on a fresh clone before any register', record: 'return value, error text, exit code' },
  // From refactor/dyno-service-shared at 070e2844c (final build):
  { id: 'update-agent-never-answers', question: 'Update 1.0.0 to 1.1.0 with the agent killed so it never answers: version is not recorded and the next start repairs again?', how: 'update, `pkill -9 -x dyno-agent` (and keep it down), read the recorded version in the settings/state file, start again', record: 'recorded version after the update, whether the next start repaired again' },
  { id: 'upgrade-item-off-first-run', question: 'First run on an upgrade with the item switched off: does Login Items open once and not on later starts?', how: 'switch the Login Items entry off, update, first start, second start; count System Settings opens in the unified log', record: 'opens at the first and at the later starts' },
  { id: 'switch-on-returns-with-agent-lock', question: 'Does the Settings switch on return at once while the app\'s own agent.lock holds the agent?', how: 'hold agent.lock with the running agent, press the switch, time the return', record: 'ms to return, card state' },
  { id: 'downgrade-refused-job-repaired', question: 'Is a downgrade with a refused job repaired?', how: 'downgrade with launchd refusing the job (78), start, read the job state', record: 'job state before and after the start' },
  { id: 'dmg-run-no-removal', question: 'App run from a disk image: is the running agent left alone and does the log show the location message?', how: 'run the app from a mounted DMG while the installed agent runs; `pgrep` the agent, grep the app log for the location message', record: 'agent pid before and after, the log line' },
  { id: 'resume-lagging-register-no-second-banner', question: 'Resume after a lagging register: no second "Background Items Added"?', how: 'register, force a resume, count the notices in `log show`', record: 'notice count' },
  { id: 'first-ten', question: 'The branch\'s first ten: fresh install; update with healthy agent; update where launchd refuses (78); switched-off item through update; Quit and stop then same version, then new version, then with the item switched off; toggle including a translocated path; `lgit` and `rsto` start in tray; Windows Run entry and Linux XDG autostart read back.', how: 'one step per case with its own result, read through `launchctl print`, `sfltool dumpbtm`, the app log and, on Windows and Linux, the Run key and ~/.config/autostart', record: 'per case: the state read and the log line' },
];
