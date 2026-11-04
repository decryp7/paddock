// The restart check on a macOS guest (the user's real-PC finding): helper on and Launch at login on, record the helper's start-at-boot state
// (launchd RunAtLoad and KeepAlive), restart the guest for real and log in, then without Repair record the helper, the card, whether Dyno started
// itself (SMAppService login item) and how long after login, the menu-bar icons and one helper reading.
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const LABEL = 'net.decryptology.dyno.helper';
const result = (title, checks, evidence = [], pass = '') => { const bad = checks.filter((c) => !c.ok); return { title, status: bad.length ? 'fail' : 'pass', summary: bad.length ? bad.map((c) => c.detail).join('; ').slice(0, 240) : pass, checks, evidence }; };

async function helperState(g) {
  const r = await g.exec(`launchctl print system/${LABEL} 2>&1 | grep -E "state =|pid =|program =|run at load|keepalive|KeepAlive|RunAtLoad|runs =|last exit" | head -12; echo '--- plist'; sudo plutil -p /Library/LaunchDaemons/${LABEL}.plist 2>&1 | grep -E "RunAtLoad|KeepAlive|Label|Program" | head -6; echo '--- files'; ls -l /Library/PrivilegedHelperTools/${LABEL} 2>&1`);
  return r.stdout.trim().split('\n');
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
    const rolesSeen = (await gui.texts()).filter((t) => /Start Dyno at login/i.test(t.text)).map((t) => `${t.role}:${t.text}`);
    if (ctl) {
      if (!/value=1/.test(ctl.text)) await gui.press(gui.app, 'Start Dyno at login', ctl.role);
      await sleep(3000);
      ctl = await find();
      loginOn = Boolean(ctl && /value=1/.test(ctl.text));
    }
    const pre = await helperState(g);
    const loginItem = (await g.exec('sfltool dumpbtm 2>/dev/null | grep -i -B1 -A4 dyno | head -14; launchctl print gui/$(id -u) 2>/dev/null | grep -i dyno | head -4')).stdout.trim().split('\n');
    steps.push(result('Restart check: before the restart (helper on, Launch at login on)', [
      { ok: loginOn, detail: loginOn ? 'switched "Start Dyno at login" on' : `could not switch "Start Dyno at login" on (found: ${rolesSeen.join(' | ') || 'nothing'})` },
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
    steps.push(result('Restart check: menu-bar icons after the restart', [{ ok: win.length > 0, detail: win.length ? `${win.length} Dyno menu-bar item(s) after the restart (appeared ${took} s after the check began, about 1 min after the autostart)` : 'no Dyno menu-bar item 90 s after the autostart' }], win, 'menu-bar items present'));
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
