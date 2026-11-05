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
