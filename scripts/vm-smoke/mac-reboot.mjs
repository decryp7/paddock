// The restart check on a macOS guest (the user's real-PC finding): helper on and Launch at login on, record the helper's start-at-boot state
// (launchd RunAtLoad and KeepAlive), restart the guest for real and log in, then without Repair record the helper, the card, whether Dyno started
// itself (SMAppService login item) and how long after login, the menu-bar icons and one helper reading.
import { menuBarFacts, judgeMenuBar } from './mac-menubar.mjs';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const LABEL = 'net.decryptology.dyno.helper';
const result = (title, checks, evidence = [], pass = '') => { const bad = checks.filter((c) => !c.ok); return { title, status: bad.length ? 'fail' : 'pass', summary: bad.length ? bad.map((c) => c.detail).join('; ').slice(0, 240) : pass, checks, evidence }; };

async function helperState(g) {
  const r = await g.exec(`launchctl print system/${LABEL} 2>&1 | grep -E "state =|pid =|program =|run at load|keepalive|KeepAlive|RunAtLoad|runs =|last exit" | head -12; echo '--- plist'; sudo plutil -p /Library/LaunchDaemons/${LABEL}.plist 2>&1 | grep -E "RunAtLoad|KeepAlive|Label|Program" | head -6; echo '--- files'; ls -l /Library/PrivilegedHelperTools/${LABEL} 2>&1`);
  return r.stdout.trim().split('\n');
}

/** Dyno's autosave names as the app log states them ("autosave name net.decryptology.dyno.menubar.<item>"), in first-seen order. */
export function autosaveNames(appLog) {
  return [...new Set([...appLog.matchAll(/autosave name (net\.decryptology\.dyno\.menubar\.[A-Za-z0-9-]+)/g)].map((m) => m[1]))];
}

/**
 * The real "shown" test on macOS 26: an item is shown when CGWindowList (on-screen only, `axdrive windows` TSV: owner, pid, layer,
 * title, bounds) has a window owned by Control Center whose title is the item's autosave name and whose bounds have a width and a
 * height. Returns one row per wanted name, plus any Dyno window seen that the log did not name.
 */
export function trayShown(windowsTsv, names) {
  const rows = windowsTsv.split('\n').map((l) => l.split('\t')).filter((c) => c.length >= 5);
  const dyno = rows.filter((c) => /^net\.decryptology\.dyno\.menubar\./.test(c[3]));
  const sized = (b) => { const m = /(\d+)x(\d+)$/.exec(b ?? ''); return Boolean(m && Number(m[1]) > 0 && Number(m[2]) > 0); };
  const out = names.map((name) => {
    const w = dyno.find((c) => c[3] === name);
    return { name, shown: Boolean(w && w[0] === 'Control Center' && sized(w[4])), owner: w?.[0] ?? null, bounds: w?.[4] ?? null };
  });
  for (const c of dyno) if (!names.includes(c[3])) out.push({ name: c[3], shown: c[0] === 'Control Center' && sized(c[4]), owner: c[0], bounds: c[4], unnamed: true });
  return out;
}

/** From Dyno's log text (the JSON lines' messages): the last start signal, every tray check line and the count of blocked-host lines. */
export function trayLogFacts(appLog) {
  const msgs = appLog.split('\n').map((l) => /"@m":"((?:[^"\\]|\\.)*)"/.exec(l)?.[1] ?? l).filter(Boolean);
  return {
    startSignal: msgs.filter((m) => /^start signal:/.test(m)).at(-1) ?? null,
    checks: msgs.filter((m) => /^tray check #\d+/.test(m)),
    lastSummary: msgs.filter((m) => /^tray check #\d+: \d+ wanted/.test(m)).at(-1) ?? null,
  };
}

/** The ControlCenter log lines about Dyno's status items: how many, and how many say the host was blocked or asked not to be visible. */
export function controlCenterFacts(ccLog) {
  const lines = ccLog.split('\n').filter((l) => /dyno/i.test(l));
  const blocked = lines.filter((l) => /blocked list|blocked host|not to be visible|visibility=0/i.test(l));
  const tracked = lines.filter((l) => /Starting to track host/.test(l));
  const ephemeral = lines.filter((l) => /ephemaral instance|ephemeral instance/i.test(l));
  return { lines: lines.length, blocked, tracked: tracked.length, ephemeral: ephemeral.length };
}

/**
 * Everything step 7 needs to name a cause (ER-27): the shown test per autosave name, the app's start signal and tray checks,
 * ControlCenter's pid and start time and its log about Dyno since the boot. Read-only on the guest.
 */
async function loginDiag(g, label) {
  const app = (await g.exec('cat "$HOME/Library/Logs/Dyno"/dyno.*.log 2>/dev/null | grep -E "autosave name|start signal:|tray check #|started hidden|Started in the tray|Main window built|starting \\(OS"')).stdout;
  const names = autosaveNames(app);
  const windows = (await g.exec('/usr/local/bin/axdrive windows')).stdout;
  const shown = trayShown(windows, names);
  const facts = trayLogFacts(app);
  const cc = (await g.exec('ps -o pid=,lstart= -p "$(pgrep -x ControlCenter | head -1)" 2>&1; echo "--- boot $(sysctl -n kern.boottime)"; echo "--- now $(date)"')).stdout.trim();
  const ccLog = (await g.exec(`log show --last 10m --info --debug --style compact --predicate 'process == "ControlCenter" AND eventMessage CONTAINS[c] "dyno"' 2>&1 | grep -v -E "runningboard|TCC:access" | tail -400`, { timeoutMs: 180_000 })).stdout;
  const ccf = controlCenterFacts(ccLog);
  const ccKey = ccLog.split('\n').filter((l) => /Host properties initialized|Starting to track host|Stopping tracking|blocked|not to be visible|visibility|ephem|Host id changed|Removing displayables/i.test(l));
  return {
    shown,
    facts,
    ccf,
    lines: [
      `--- ${label}: shown test (on-screen Control Center window named after each autosave name)`,
      ...(shown.length ? shown.map((s) => `${s.shown ? 'shown' : 'NOT shown'}\t${s.name}\t${s.owner ?? '-'}\t${s.bounds ?? '-'}${s.unnamed ? '\t(not named in the app log)' : ''}`) : ['(the app log names no autosave name)']),
      `--- ${label}: app start signal: ${facts.startSignal ?? '(none logged)'}`,
      `--- ${label}: tray checks (${facts.checks.length} line(s)); last summary: ${facts.lastSummary ?? '(none)'}`,
      ...facts.checks.slice(-40),
      `--- ${label}: ControlCenter pid and start time`,
      ...cc.split('\n'),
      `--- ${label}: ControlCenter log about Dyno, last 10 min: ${ccf.lines} line(s), ${ccf.tracked} "Starting to track host", ${ccf.ephemeral} ephemeral, ${ccf.blocked.length} blocked/visibility-0`,
      ...ccf.blocked.slice(0, 20).map((l) => `BLOCKED ${l.slice(0, 300)}`),
      ...ccKey.slice(-120).map((l) => l.slice(0, 300)),
    ],
  };
}

/**
 * How step 5 treats "Start Dyno at login" (env `VM_SMOKE_LOGIN_SWITCH`): `press` (default) switches it on in this session when it
 * is off; `leave` only records it, so the restart runs on the registration the app made by itself at its first start.
 */
export function loginSwitchMode(env = process.env) {
  const v = env.VM_SMOKE_LOGIN_SWITCH ?? 'press';
  if (!['press', 'leave'].includes(v)) throw new Error(`VM_SMOKE_LOGIN_SWITCH=${v}: press (default) or leave`);
  return v;
}

export function macRebootCheck({ waitReady }) {
  return async ({ gui, check }) => {
    const g = gui.g;
    const steps = [];
    // Launch at login on (Settings > Dyno): find the control whatever its role is.
    const nav = await gui.openSettingsSection('^Dyno$');
    await sleep(2000);
    const find = async () => (await gui.texts()).find((t) => /Start Dyno at login/i.test(t.text) && ['AXCheckBox', 'AXSwitch', 'AXButton', 'AXRadioButton', 'AXToggle'].includes(t.role));
    let ctl = nav.ok ? await find() : null;
    let loginOn = false;
    const switchMode = loginSwitchMode();
    const switchBefore = ctl?.value ?? null;
    let pressed = false;
    const rolesSeen = (await gui.texts()).filter((t) => /Start Dyno at login/i.test(t.text)).map((t) => `${t.role}:${t.raw}`);
    if (ctl) {
      if (ctl.value !== '1' && switchMode === 'press') { await gui.press(gui.app, 'Start Dyno at login', ctl.role); pressed = true; }
      await sleep(3000);
      ctl = await find();
      loginOn = Boolean(ctl && ctl.value === '1');
    }
    const pre = await helperState(g);
    const loginItem = (await g.exec('sfltool dumpbtm 2>/dev/null | grep -i -B1 -A4 dyno | head -14; launchctl print gui/$(id -u) 2>/dev/null | grep -i dyno | head -4')).stdout.trim().split('\n');
    steps.push(result('Restart check: before the restart (helper on, Launch at login on)', [
      { ok: loginOn, detail: loginOn ? `"Start Dyno at login" is on (mode ${switchMode}; it was ${switchBefore === '1' ? 'already on' : 'off'}${pressed ? ', pressed in this session' : ''})` : `"Start Dyno at login" is not on (mode ${switchMode}, value before ${switchBefore ?? 'not found'}; found: ${rolesSeen.join(' | ') || 'nothing'})` },
      { ok: /state = running/.test(pre.join('\n')), detail: `helper before the restart: ${pre.find((l) => /state =/.test(l))?.trim() ?? 'not loaded'}` },
      { ok: /RunAtLoad|run at load|KeepAlive|keepalive/i.test(pre.join('\n')), detail: 'the helper job is declared to start at boot and be kept alive (RunAtLoad / KeepAlive in the plist or job)' },
    ], [...pre, '--- login item', ...loginItem], 'helper running with RunAtLoad and KeepAlive'));
    // The restart.
    const boot0 = (await g.exec('sysctl -n kern.boottime')).stdout.trim();
    await g.exec('(sleep 2; sudo /sbin/reboot) >/dev/null 2>&1 &', { timeoutMs: 30_000 });
    await sleep(40_000);
    let up = false;
    for (let i = 0; i < 80 && !up; i++) { up = (await waitReady()) && (await g.exec('sysctl -n kern.boottime')).stdout.trim() !== boot0; if (!up) await sleep(4000); }
    if (!up) { steps.push(result('Restart check: the guest did not come back', [{ ok: false, detail: 'no guest agent after the restart (5 min)' }])); return steps; }
    await sleep(25_000);
    const t0 = Date.now();
    const post = await helperState(g);
    const procs = (await g.exec('ps -axo pid=,etime=,lstart=,command= | grep -E "Dyno.app/Contents/MacOS/(dyno-app|dyno-agent)" | grep -v grep')).stdout.trim();
    const login = (await g.exec('last -1 admin 2>&1 | head -2; who | head -3; sysctl -n kern.boottime')).stdout.trim();
    const autostart = /dyno-app/.test(procs);
    const boot1 = (await g.exec('sysctl -n kern.boottime')).stdout.trim();
    steps.push(result('Restart check: Dyno and the helper after the restart, no Repair', [
      { ok: boot1 !== boot0, detail: boot1 !== boot0 ? `the guest really restarted (boot time ${boot0.replace(/\s+/g, ' ')} -> ${boot1.replace(/\s+/g, ' ')})` : 'the guest did NOT restart (boot time unchanged), so nothing below proves anything' },
      { ok: /state = running/.test(post.join('\n')), detail: `helper after the restart: ${post.find((l) => /state =/.test(l))?.trim() ?? 'not loaded'}` },
      { ok: autostart, detail: autostart ? 'Dyno started by itself after login (SMAppService login item)' : 'Dyno did not start by itself after login' },
    ], ['--- Dyno processes (pid etime lstart)', ...procs.split('\n'), '--- login/boot', ...login.split('\n'), '--- helper', ...post], 'helper running, Dyno autostarted'));
    // The menu-bar items: poll up to 90 s after the autostart and say how long they took.
    const startedAt = Date.now();
    let win = [];
    for (let i = 0; i < 30 && !win.length; i++) { win = (await g.exec('/usr/local/bin/axdrive windows')).stdout.split('\n').filter((l) => /net\.decryptology\.dyno\.menubar/.test(l)); if (!win.length) await sleep(3000); }
    const took = Math.round((Date.now() - startedAt) / 1000);
    // ER-27: the facts that name the cause, pass or fail.
    const d1 = await loginDiag(g, win.length ? 'after the items appeared' : '90 s after the autostart');
    let diag = [...d1.lines];
    if (!win.length) {
      // Which is it: no status items at all, items under another owner, or an app that is not running its tray? Collect the facts, then ask the app for its window.
      const owners = (await g.exec('/usr/local/bin/axdrive windows | cut -f1,4 | sort | uniq -c | sort -rn | head -25')).stdout.trim();
      const lsapp = (await g.exec('lsappinfo list 2>&1 | grep -i -B1 -A10 "dyno" | head -40')).stdout.trim();
      const ccItems = (await g.exec('/usr/local/bin/axdrive tree ControlCenter 6 2>&1 | grep -i -E "dyno|menu bar item|AXMenuBarItem|AXStatusItem" | head -20; echo --- SystemUIServer; /usr/local/bin/axdrive tree SystemUIServer 6 2>&1 | grep -i -E "dyno|AXMenuBarItem" | head -10')).stdout.trim();
      const log = (await g.exec('for d in "$HOME/Library/Logs/Dyno" "$HOME/Library/Application Support/Dyno/logs" "$HOME/Library/Application Support/Dyno"; do ls -la "$d" 2>&1 | tail -4; grep -h -i -E "tray|tooltip|status item|menubar" "$d"/*.log 2>/dev/null | tail -10; done')).stdout.trim();
      await g.exec('open -a /Applications/Dyno.app'); await sleep(25_000);
      const after = (await g.exec('/usr/local/bin/axdrive windows')).stdout.split('\n').filter((l) => /dyno/i.test(l));
      const items2 = after.filter((l) => /menubar/.test(l));
      const d2 = await loginDiag(g, 'after "open -a Dyno"');
      diag = [...diag, ...d2.lines, '--- window owners (count, owner, title)', ...owners.split('\n'), '--- lsappinfo', ...lsapp.split('\n'), '--- Control Center / SystemUIServer items', ...ccItems.split('\n'), '--- app log tray lines', ...log.split('\n'), `--- after "open -a Dyno" (25 s): ${after.length} Dyno window(s), ${items2.length} menu-bar item(s)`, ...after];
    }
    const wanted = d1.shown.filter((s) => !s.unnamed);
    // A guest without a menu bar measures nothing: look for the Menubar window first, always take a screenshot, record dyno-app and its crash reports.
    const mb = await menuBarFacts(g);
    const shot = await gui.shot('menu-bar-after-restart');
    const j = judgeMenuBar({ ...mb, items: win.length });
    const mbChecks = [...j.checks];
    if (mb.menubar && win.length) mbChecks.push({ ok: wanted.length > 0 && wanted.every((s) => s.shown), detail: `shown test: ${wanted.filter((s) => s.shown).length} of ${wanted.length} autosave name(s) on screen under Control Center; start signal ${d1.facts.startSignal ?? 'none'}; ControlCenter blocked lines ${d1.ccf.blocked.length}` });
    const mbBad = mbChecks.filter((c) => !c.ok);
    steps.push({ title: 'Restart check: menu-bar icons after the restart', status: mbBad.length || j.status === 'fail' ? 'fail' : j.status, summary: j.status === 'not-measured' ? j.summary : mbBad.length ? mbBad.map((c) => c.detail).join('; ').slice(0, 240) : j.summary, checks: mbChecks, evidence: [`screenshot: ${shot}`, `appeared ${win.length ? `${took} s after the check began` : 'never within 90 s'}`, ...win, ...diag] });
    // An autostarted Dyno may be hidden in the tray: bring its window up the way a click on the Dock or the menu bar does.
    await g.exec('open -a /Applications/Dyno.app'); await sleep(20_000);
    await gui.closeSettings();
    const opened = await gui.press(gui.app, '^Processes$', 'AXLink');
    await sleep(4000);
    const text = (await gui.texts()).map((t) => t.text).join(' ');
    await gui.openSettingsSection('^Dyno$'); await sleep(2500);
    const card = (await gui.texts()).map((t) => t.text).filter((x) => /^(On|Off|Update available|Not running|Needs repair|Not trusted)$/.test(x));
    steps.push(result('Restart check: the card and a helper reading after the restart', [
      { ok: card.includes('On'), detail: `card state(s): ${card.join(', ') || 'none found'}` },
      { ok: opened && /Read by the administrator helper/i.test(text), detail: /Read by the administrator helper/i.test(text) ? 'Processes shows "Read by the administrator helper"' : 'Processes does not show the helper chip after the restart' },
    ], [`Processes text: ${text.slice(0, 500)}`], 'card On and a reading works'));
    void t0; void check;
    return steps;
  };
}
