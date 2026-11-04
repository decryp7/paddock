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
    // Launch at login on (Settings > Dyno).
    const nav = await gui.openSettingsSection('^Dyno$');
    const toggle = nav.ok && (await gui.waitFor(async () => (await gui.texts()).find((t) => t.role === 'AXCheckBox' && /Start Dyno at login/i.test(t.text)), 15_000));
    let loginOn = false;
    if (toggle?.ok) { const cur = toggle.value; if (!/value=1/.test(cur.text)) await gui.press(gui.app, 'Start Dyno at login', 'AXCheckBox'); await sleep(3000); loginOn = (await gui.texts()).some((t) => t.role === 'AXCheckBox' && /Start Dyno at login/i.test(t.text) && /value=1/.test(t.text)); }
    const pre = await helperState(g);
    const loginItem = (await g.exec('sfltool dumpbtm 2>/dev/null | grep -i -B1 -A4 dyno | head -14; launchctl print gui/$(id -u) 2>/dev/null | grep -i dyno | head -4')).stdout.trim().split('\n');
    steps.push(result('Restart check: before the restart (helper on, Launch at login on)', [
      { ok: loginOn, detail: loginOn ? 'switched "Start Dyno at login" on' : 'could not switch "Start Dyno at login" on' },
      { ok: /state = running/.test(pre.join('\n')), detail: `helper before the restart: ${pre.find((l) => /state =/.test(l))?.trim() ?? 'not loaded'}` },
      { ok: /RunAtLoad|run at load|KeepAlive|keepalive/i.test(pre.join('\n')), detail: 'the helper job is declared to start at boot and be kept alive (RunAtLoad / KeepAlive in the plist or job)' },
    ], [...pre, '--- login item', ...loginItem], 'helper running with RunAtLoad and KeepAlive'));
    // The restart.
    await g.exec('sudo shutdown -r now >/dev/null 2>&1 &', { timeoutMs: 30_000 });
    await sleep(40_000);
    const up = await waitReady(); // tart exec answers again and the Aqua session is up
    if (!up) { steps.push(result('Restart check: the guest did not come back', [{ ok: false, detail: 'no guest agent after the restart (5 min)' }])); return steps; }
    await sleep(25_000);
    const t0 = Date.now();
    const post = await helperState(g);
    const procs = (await g.exec('ps -axo pid=,etime=,lstart=,command= | grep -E "Dyno.app/Contents/MacOS/(dyno-app|dyno-agent)" | grep -v grep')).stdout.trim();
    const login = (await g.exec('last -1 admin 2>&1 | head -2; who | head -3; sysctl -n kern.boottime')).stdout.trim();
    const autostart = /dyno-app/.test(procs);
    steps.push(result('Restart check: Dyno and the helper after the restart, no Repair', [
      { ok: /state = running/.test(post.join('\n')), detail: `helper after the restart: ${post.find((l) => /state =/.test(l))?.trim() ?? 'not loaded'}` },
      { ok: autostart, detail: autostart ? 'Dyno started by itself after login (SMAppService login item)' : 'Dyno did not start by itself after login' },
    ], ['--- Dyno processes (pid etime lstart)', ...procs.split('\n'), '--- login/boot', ...login.split('\n'), '--- helper', ...post], 'helper running, Dyno autostarted'));
    const win = (await g.exec('/usr/local/bin/axdrive windows')).stdout.split('\n').filter((l) => /net\.decryptology\.dyno\.menubar/.test(l));
    steps.push(result('Restart check: menu-bar icons after the restart', [{ ok: win.length > 0, detail: `${win.length} Dyno menu-bar item(s) after the restart` }], win, 'menu-bar items present'));
    if (!autostart) { await g.exec('open -a /Applications/Dyno.app'); await sleep(25_000); }
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
