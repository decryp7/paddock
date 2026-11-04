// Dyno's notification-area icons on the Windows guest, found the way a person sees them. Dyno 1.1's icons are named by their tooltip,
// which starts with the metric ("CPU 34%", "Memory: the agent is not running"), never with "Dyno" (tray_model::item_tooltip), so a
// search for "Dyno" finds the buttons of Dyno's own window instead (the 7538 restart check counted "Dyno" and "Start Dyno at login").
// Here an icon is a UI Automation button whose top-level window belongs to explorer (the taskbar Shell_TrayWnd or the overflow flyout)
// and whose name starts with the metric of one of the items the app log says it created.

/** The actor command (win-act.ps1 `trayicons`): every named button with its centre, class, top-level class and top-level process. */
export const TRAY_ICONS_CMD = 'trayicons .';

/** The top-level window classes of the notification area: the taskbar, a second taskbar, and the overflow flyout (Windows 11 and 10). */
const TASKBAR = new Set(['Shell_TrayWnd', 'Shell_SecondaryTrayWnd']);
const OVERFLOW = new Set(['TopLevelWindowForOverflowXamlIsland', 'NotifyIconOverflowWindow']);

/** The metrics of the items the app log says it created ("tray icon dyno-x created (metric CPU, ..."), in first-seen order. */
export function metricsFromLog(appLog) {
  return [...new Set([...appLog.matchAll(/tray icon dyno-[A-Za-z0-9-]+ created \(metric ([^,]+),/g)].map((m) => m[1].trim()))];
}

const escape = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/**
 * Dyno's icons among the actor's rows (`name \t x,y \t class \t top-level class \t top-level process`): explorer-owned, in the taskbar or
 * the overflow, named "<metric> ..." or "<metric>: ..." for one of `metrics`. Anything in a window of Dyno itself is never an icon.
 */
export function dynoTrayIcons(rows, metrics) {
  const names = metrics.length ? metrics : [];
  // item_tooltip's forms only: "<metric> <reading>" (a reading starts with a digit, a sign or a dash), "<metric>: ...", "<metric> is not available ...".
  const re = names.length ? new RegExp(`^(${names.map(escape).join('|')})(: | is not available| [-+\u2013\u2014<>~.0-9])`) : null;
  return rows
    .split('\n')
    .map((l) => l.split('\t'))
    .filter((c) => c.length >= 5 && c[0].trim())
    .map(([name, at, cls, top, proc]) => ({ name: name.trim(), at: at.trim(), cls: cls.trim(), top: top.trim(), proc: proc.trim() }))
    .filter((r) => /^explorer$/i.test(r.proc) && (TASKBAR.has(r.top) || OVERFLOW.has(r.top)) && re !== null && re.test(r.name))
    .map((r) => ({ ...r, where: TASKBAR.has(r.top) ? 'taskbar' : 'overflow' }));
}

/** Dyno's log lines that explain its tray at a start (ER-27): the start, the hidden flag, every icon made, its GUID state and each check. */
export function trayLogLines(appLog) {
  return appLog
    .split('\n')
    .map((l) => {
      const m = /"@m":"((?:[^"\\]|\\.)*)"(?:,"@t":"([^"]+)")?/.exec(l);
      return m ? `${m[2] ?? ''} ${m[1]}`.trim() : '';
    })
    .filter((m) => /starting \(OS|start signal|started hidden|Started in the tray|tray sync|tray icon dyno-|tray check #|TaskbarCreated|taskbar|notification area|GUID|Main window built/i.test(m));
}

/** The PowerShell that prints Dyno's log for today (the newest dyno.*.log under the app's folders), for `ssh`. */
export const APP_LOG_PS = `$f = Get-ChildItem -Path "$env:LOCALAPPDATA\\Dyno","$env:APPDATA\\Dyno" -Recurse -Depth 3 -Filter 'dyno.*.log' -ErrorAction SilentlyContinue | Where-Object { $_.Name -notlike 'dyno-agent*' } | Sort-Object LastWriteTime | Select-Object -Last 1; if ($f) { "@@ $($f.FullName)"; Get-Content $f.FullName -ErrorAction SilentlyContinue } else { '@@ (no dyno.*.log found)' }`;
