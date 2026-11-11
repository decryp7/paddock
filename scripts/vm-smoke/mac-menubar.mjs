// The macOS menu-bar judgement. A guest may have no usable menu bar at all (AccessibilityUIServer frontmost, no clock), and then "no Dyno item" measured
// nothing. The bar is confirmed by a REAL signal: the Apple items every bar has (Clock, Control Center: `com.apple.menuextra.*` in the Accessibility tree
// of SystemUIServer / ControlCenter, seen in the results of 2026-11-04). They are the control: if the reader cannot see them it cannot see Dyno's either, so the
// step is not measured. (The earlier test looked for a Window Server window titled "Menubar"; no result file ever showed one.) Fixtures: fixtures/README.md.

/** The Apple menu extras the Accessibility text names (`AXMenuBarItem  Clock | ... | com.apple.menuextra.clock`): ['clock', 'controlcenter']. */
export function appleItems(axText) {
  return [...new Set([...String(axText ?? '').matchAll(/AXMenuBarItem\s+[^|\n]*\|(?:[^|\n]*\|)?\s*com\.apple\.menuextra\.([a-z]+)/g)].map((m) => m[1]))];
}

/** True when the reader sees at least one Apple item: the bar is up and the reader works. */
export const hasMenuBar = (axText) => appleItems(axText).length > 0;

/** Dyno's menu-bar rows of `axdrive windows` (owner, pid, layer, title, bounds). */
export function dynoItems(windowsTsv) {
  return windowsTsv.split('\n').map((l) => l.split('\t')).filter((c) => /^net\.decryptology\.dyno\.menubar\./.test(c[3] ?? ''));
}

/**
 * @param f { menubar: boolean, controls?: string[], items: number, dynoRunning: boolean, crashReports: string[] }
 * @returns { status: 'pass'|'fail'|'not-measured', summary, checks }
 */
export function judgeMenuBar(f) {
  const info = { ok: true, info: true, detail: f.menubar ? `the menu bar is up (control items seen: ${(f.controls ?? []).join(', ') || 'by the caller'})` : 'no known menu-bar item (Clock, Control Center) is visible to the reader: the bar is not up or the reader cannot see it' };
  const crash = { ok: f.dynoRunning || !f.crashReports.length, detail: f.dynoRunning ? `dyno-app is running${f.crashReports.length ? `; crash report(s) exist: ${f.crashReports.join(', ')}` : '; no crash report for it' }` : f.crashReports.length ? `dyno-app is NOT running and a crash report exists: ${f.crashReports.join(', ')} (a crash candidate for the missing items)` : 'dyno-app is not running; no crash report for it' };
  const checks = [info, crash];
  if (!f.dynoRunning && f.crashReports.length) return { status: 'fail', summary: crash.detail, checks };
  if (!f.menubar) return { status: 'not-measured', summary: 'not measured: the reader sees no known menu-bar item (Clock, Control Center), so no Dyno item could be seen either', checks };
  checks.push({ ok: f.items > 0, detail: f.items ? `${f.items} Dyno menu-bar item(s)` : 'no Dyno menu-bar item although the menu bar is up' });
  const bad = checks.filter((c) => !c.ok);
  return { status: bad.length ? 'fail' : 'pass', summary: bad.length ? bad.map((c) => c.detail).join('; ').slice(0, 240) : `${f.items} Dyno menu-bar item(s) with the menu bar up`, checks };
}

/** Reads the facts from the guest: the windows, the Apple items in the Accessibility trees, whether dyno-app runs, and crash reports named for it. */
export async function menuBarFacts(g) {
  const tsv = (await g.exec('/usr/local/bin/axdrive windows')).stdout;
  const ax = (await g.exec('/usr/local/bin/axdrive tree SystemUIServer 6 2>&1; /usr/local/bin/axdrive tree ControlCenter 6 2>&1')).stdout;
  const running = (await g.exec('pgrep -x dyno-app >/dev/null && echo yes || echo no')).stdout.trim() === 'yes';
  const crashes = (await g.exec('ls -t "$HOME/Library/Logs/DiagnosticReports" /Library/Logs/DiagnosticReports 2>/dev/null | grep -i "dyno" | head -5')).stdout.split('\n').map((l) => l.trim()).filter(Boolean);
  return { tsv, ax, menubar: hasMenuBar(ax), controls: appleItems(ax), items: dynoItems(tsv).length, dynoRunning: running, crashReports: crashes };
}

/** Brings the bar up the way a person does (Finder to the front), then waits (bounded) until a real Apple item is visible. Returns the facts of the last read. */
export async function confirmMenuBar(g, { tries = 12, sleep = (ms) => new Promise((r) => setTimeout(r, ms)) } = {}) {
  await g.exec('open -a Finder');
  let facts = null;
  for (let i = 0; i < tries; i++) {
    facts = await menuBarFacts(g);
    if (facts.menubar) return { ...facts, waited: i };
    await sleep(5000);
  }
  return { ...facts, waited: tries };
}
