// The macOS menu-bar judgement. A Tahoe guest often has NO menu bar at all (AccessibilityUIServer frontmost, no Window Server "Menubar" window, no clock in the
// screenshot): no app shows status items there, Stats included, so "no Dyno item" measured nothing. The step therefore first looks for the Menubar window and
// reports "not measured" (never FAIL) when it is absent. A Dyno that is gone after the login start with a crash report is a real finding either way.

/** True when the Window Server has a "Menubar" window (`axdrive windows` TSV: owner, pid, layer, title, bounds). */
export function hasMenuBar(windowsTsv) {
  return windowsTsv.split('\n').map((l) => l.split('\t')).some((c) => c.length >= 4 && /^Window Server$/i.test(c[0]) && c[3] === 'Menubar');
}

/** Dyno's menu-bar rows of the same TSV. */
export function dynoItems(windowsTsv) {
  return windowsTsv.split('\n').map((l) => l.split('\t')).filter((c) => /^net\.decryptology\.dyno\.menubar\./.test(c[3] ?? ''));
}

/**
 * @param f { menubar: boolean, items: number, dynoRunning: boolean, crashReports: string[] }
 * @returns { status: 'pass'|'fail'|'not-measured', summary, checks }
 */
export function judgeMenuBar(f) {
  const checks = [
    { ok: true, detail: f.menubar ? 'the guest has a menu bar (a Window Server "Menubar" window exists)' : 'the guest has NO menu bar (no Window Server "Menubar" window): status items cannot show' },
    { ok: f.dynoRunning || !f.crashReports.length, detail: f.dynoRunning ? `dyno-app is running${f.crashReports.length ? `; crash report(s) exist: ${f.crashReports.join(', ')}` : '; no crash report for it' }` : f.crashReports.length ? `dyno-app is NOT running and a crash report exists: ${f.crashReports.join(', ')} (a crash candidate for the missing items)` : 'dyno-app is not running; no crash report for it' },
  ];
  if (!f.dynoRunning && f.crashReports.length) return { status: 'fail', summary: checks[1].detail, checks };
  if (!f.menubar) return { status: 'not-measured', summary: 'not measured: guest has no menu bar', checks };
  checks.push({ ok: f.items > 0, detail: f.items ? `${f.items} Dyno menu-bar item(s)` : 'no Dyno menu-bar item although the menu bar is up' });
  const bad = checks.filter((c) => !c.ok);
  return { status: bad.length ? 'fail' : 'pass', summary: bad.length ? bad.map((c) => c.detail).join('; ').slice(0, 240) : `${f.items} Dyno menu-bar item(s) with the menu bar up`, checks };
}

/** Reads the facts from the guest: the windows, whether dyno-app runs, and crash reports named for it. */
export async function menuBarFacts(g) {
  const tsv = (await g.exec('/usr/local/bin/axdrive windows')).stdout;
  const running = (await g.exec('pgrep -x dyno-app >/dev/null && echo yes || echo no')).stdout.trim() === 'yes';
  const crashes = (await g.exec('ls -t "$HOME/Library/Logs/DiagnosticReports" /Library/Logs/DiagnosticReports 2>/dev/null | grep -i "dyno" | head -5')).stdout.split('\n').map((l) => l.trim()).filter(Boolean);
  return { tsv, menubar: hasMenuBar(tsv), items: dynoItems(tsv).length, dynoRunning: running, crashReports: crashes };
}
