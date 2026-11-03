// VM / release check (ADR 0040: shortcuts and login items start a GUI-subsystem program): the user's Run key, as `reg query
// HKCU\Software\Microsoft\Windows\CurrentVersion\Run` prints it after Start at login was switched on, must hold Dyno's app entry
// (`dyno-app.exe --hidden`, which starts the agent itself) and no entry that starts only `dyno-agent.exe` (Dyno 1.0.0 left
// `Dyno = dyno-agent.exe run`: no tray icon, no window, a console).
//   node scripts/check-logon-entry.mjs <file with the reg query output>
// VM-only check: it runs in no CI workflow (the Run key is the user's, set by the app at run time); the Windows VM runbook
// (docs/testing/windows-gui-session.md) runs it on the `reg query` output after Start at login is switched on and after a restart.
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

/** The Dyno values of a `reg query` listing: [name, data]. */
export function dynoValues(regOutput) {
  const out = [];
  for (const line of regOutput.split(/\r?\n/)) {
    const m = /^\s+(.+?)\s+REG_(?:SZ|EXPAND_SZ)\s+(.*?)\s*$/.exec(line);
    if (m && /dyno/i.test(m[1] + m[2])) out.push([m[1], m[2]]);
  }
  return out;
}

/** What is wrong with the logon entries (empty when the app's hidden entry is there and nothing starts only the agent). */
export function problems(regOutput) {
  const values = dynoValues(regOutput);
  const found = [];
  for (const [name, data] of values) {
    if (/dyno-agent/i.test(data) && !/dyno-app/i.test(data)) found.push(`the logon entry "${name}" starts only dyno-agent (${data}): no tray icon, no window`);
  }
  if (!values.some(([, data]) => /dyno-app\.exe"?\s+--hidden/i.test(data))) found.push('no logon entry starts dyno-app.exe --hidden');
  return found;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  const found = problems(readFileSync(process.argv[2], 'utf8'));
  for (const p of found) console.log(`::error::${p}. Next step: the app's own Run value is the only logon entry; see Dyno's login.rs.`);
  if (found.length === 0) console.log('logon entries ok');
  process.exit(found.length === 0 ? 0 : 1);
}
