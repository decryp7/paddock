// The GUI and idle release checks of Dyno 1.1.0 on a macOS guest (docs: README.md "Power checks"). The app's own window is driven through
// the Accessibility API with `axdrive` (installed in the test image; source in guest/axdrive.swift), the system's password sheet the
// same way, and real pointer drags through the VM's own VNC server (vncdotool). Every step waits on a state with a guard; every verdict
// comes from the OS (launchd, the filesystem, pmset) or from the app's own text, never from a screenshot.
import { parseTree, rawTextOf } from './ax-tree.mjs';

const sq = (s) => `'${String(s).replace(/'/g, `'\\''`)}'`;
export const HELPER_LABEL = 'net.decryptology.dyno.helper';
export const HELPER_FILE = `/Library/PrivilegedHelperTools/${HELPER_LABEL}`;
/** Words the Dyno UI uses (apps/dyno/app/src/app/features/settings/helper-model.ts). */
export const UI = { turnOn: '^Turn on administrator readings$', turnOff: '^Turn off administrator readings$', on: '^On$' };

export class MacGui {
  /** @param guest {exec, now, sleep}  @param o {app, ax, password, evidence(name, guestPath), log} */
  constructor(guest, { app = 'Dyno', ax = '/usr/local/bin/axdrive', password = 'admin', fetch = null, log = () => {}, intervalMs = 1000 } = {}) {
    Object.assign(this, { g: guest, app, ax, password, fetch, log, interval: intervalMs, shots: 0 });
  }

  axCmd(args) { return `${this.ax} ${args}`; }
  async has(app, re, role = 'AXButton', nth = 0) { return (await this.g.exec(this.axCmd(`has ${sq(app)} ${sq(re)} ${role} ${nth}`))).code === 0; }
  /** `nth`: the n-th match (0-based), for a name that two elements share (two dashboard widgets called CPU). */
  async press(app, re, role = 'AXButton', nth = 0) { return (await this.g.exec(this.axCmd(`press ${sq(app)} ${sq(re)} ${role} ${nth}`))).code === 0; }

  async waitFor(fn, guardMs) {
    const t0 = this.g.now();
    for (;;) {
      const v = await fn();
      if (v) return { ok: true, value: v };
      if (this.g.now() - t0 >= guardMs) return { ok: false, value: v };
      await this.g.sleep(this.interval);
    }
  }

  /** Texts of the app window (static texts, buttons, links), from the AX tree. */
  async texts(app = this.app, depth = 40) {
    const r = await this.g.exec(this.axCmd(`tree ${sq(app)} ${depth}`), { timeoutMs: 60_000 });
    return parseTree(r.stdout);
  }

  async shot(name) {
    const file = `/tmp/ev/${String(++this.shots).padStart(2, '0')}-${name}.png`;
    await this.g.exec(`mkdir -p /tmp/ev && screencapture -x ${file}`);
    const saved = this.fetch ? await this.fetch(file) : null;
    return saved ?? file;
  }

  /** Fills the system's password sheet (coreautha on macOS 26, SecurityAgent before) and presses OK. The password is never logged. */
  async answerPasswordSheet(guardMs = 60_000) {
    let who = null;
    const seen = await this.waitFor(async () => { for (const p of ['coreautha', 'SecurityAgent']) if (await this.has(p, '^(OK|Modify Settings|Install Helper)$')) { who = p; return p; } return null; }, guardMs);
    if (!seen.ok) return { ok: false, detail: 'no password sheet appeared' };
    const pre = await this.shot('password-sheet');
    const set = await this.g.exec(`printf '%s\\n' ${sq(this.password)} | ${this.axCmd(`setfield ${who} 1`)}`);
    if (set.code !== 0) return { ok: false, detail: `could not fill the password field of ${who}`, shot: pre };
    const ok = await this.press(who, '^(OK|Modify Settings|Install Helper)$');
    if (!ok) return { ok: false, detail: `no OK button in ${who}`, shot: pre };
    const gone = await this.waitFor(async () => !(await this.has(who, '^(OK|Modify Settings|Install Helper)$')), 60_000);
    return { ok: gone.ok, detail: gone.ok ? `the ${who} password sheet was answered` : 'the sheet stayed (wrong password?)', shot: pre };
  }

  async helperInstalled() {
    const r = await this.g.exec(`launchctl print system/${HELPER_LABEL} >/dev/null 2>&1; echo $?; ls ${HELPER_FILE} >/dev/null 2>&1; echo $?`);
    const [job, file] = r.stdout.trim().split('\n').map(Number);
    return { job: job === 0, file: file === 0 };
  }

  /** Brings the app to the front (a restarted app is not frontmost, and menu items of a background app do not act). */
  async activate() { await this.g.exec(`osascript -e ${sq(`tell application "${this.app}" to activate`)}`); await this.g.sleep(1000); }

  /**
   * The main window is on screen. After a restart Dyno starts hidden (`--hidden`: tray / menu bar only), and its pages and Settings rows have nothing to
   * press. A second `open -a` is the Dock-click reopen, which shows the window of the running app. Waited on the window (the Dashboard link), up to 30 s.
   */
  async ensureMainWindow() {
    const shown = () => this.has(this.app, '^Dashboard$', 'AXLink');
    if (await shown()) return true;
    await this.g.exec(`open -a ${sq(`/Applications/${this.app}.app`)}`);
    return (await this.waitFor(shown, 30_000)).ok;
  }

  /** Dyno > Settings… (the app menu item opens the Settings window), then the section button of that window. */
  async openSettingsSection(sectionRe) {
    await this.ensureMainWindow();
    if (!(await this.has(this.app, '^Settings$', 'AXWindow'))) {
      // An AXPress on a menu item of an app that is not frontmost is accepted and does nothing (row 25 of 2026-11-05: after the MCP row restarted the app with `open`).
      await this.activate();
      const m = await this.press(this.app, '^Settings…', 'AXMenuItem');
      if (!m) return { ok: false, detail: 'no "Settings…" item in the Dyno menu' };
      const w = await this.waitFor(() => this.has(this.app, '^Settings$', 'AXWindow'), 20_000);
      if (!w.ok) return { ok: false, detail: 'the Settings window did not open within 20 s' };
    }
    const s = await this.waitFor(() => this.press(this.app, sectionRe, 'AXButton'), 15_000);
    return { ok: s.ok, detail: s.ok ? `opened Settings > ${sectionRe.replace(/[\^$]/g, '')}` : `no "${sectionRe}" section in Settings` };
  }

  /** Settings > Administrator readings > Turn on: password sheet, card ends "On", launchd job and file exist. */
  async helperOn() {
    const title = 'Admin helper: Turn on (password sheet), card "On"';
    const checks = [];
    const evidence = [];
    const before = await this.helperInstalled();
    checks.push({ ok: !before.job && !before.file, detail: `before: launchd job ${before.job ? 'present' : 'absent'}, file ${before.file ? 'present' : 'absent'} (both must be absent)` });
    const nav = await this.openSettingsSection('^Dyno$');
    checks.push({ ok: nav.ok, detail: nav.detail });
    const found = await this.waitFor(() => this.has(this.app, UI.turnOn), 20_000);
    checks.push({ ok: found.ok, detail: found.ok ? 'the card offers "Turn on administrator readings"' : 'no "Turn on administrator readings" button within 20 s' });
    if (!found.ok) { evidence.push(`screenshot: ${await this.shot('helper-no-button')}`); return this.finish(title, checks, evidence); }
    await this.press(this.app, UI.turnOn);
    const sheet = await this.answerPasswordSheet();
    checks.push({ ok: sheet.ok, detail: sheet.detail });
    if (sheet.shot) evidence.push(`screenshot: ${sheet.shot}`);
    const on = await this.waitFor(async () => { const h = await this.helperInstalled(); return h.job && h.file ? h : null; }, 90_000);
    checks.push({ ok: on.ok, detail: on.ok ? `macOS has the helper: launchd job system/${HELPER_LABEL} and ${HELPER_FILE}` : 'launchd job or file still missing 90 s after the password' });
    const badge = await this.waitFor(async () => (await this.texts()).some((t) => new RegExp(UI.on).test(t.text)), 60_000);
    const words = badge.ok ? 'On' : (await this.texts()).filter((t) => /^(Off|Update available|Not running|Needs repair|Not trusted|Turning on)$|helper version|cannot use/.test(t.text)).map((t) => t.text).join(' / ');
    checks.push({ ok: badge.ok, detail: badge.ok ? 'the card reads "On"' : `the card never read "On": it says ${words || 'nothing recognisable'}` });
    evidence.push(`screenshot: ${await this.shot('helper-on')}`);
    // The Settings window is a second window over the main one: left open it covers the Power, Schedule and Wake pages of the next steps (steps 5-9 of the 2026-11-05 run).
    await this.closeSettings();
    return this.finish(title, checks, evidence, 'password sheet answered, card "On", launchd job and file present');
  }

  /** Turn off administrator readings removes the helper: launchd job and file gone, card "Off". */
  async helperOff() {
    const title = 'Admin helper: Turn off removes it';
    const checks = [];
    const evidence = [];
    const nav = await this.openSettingsSection('^Dyno$');
    checks.push({ ok: nav.ok, detail: nav.detail });
    const off = await this.waitFor(() => this.press(this.app, UI.turnOff), 15_000).then((r) => r.ok);
    checks.push({ ok: off, detail: off ? 'pressed "Turn off administrator readings"' : 'no "Turn off administrator readings" button' });
    if (off) {
      const extra = await this.waitFor(async () => (await this.has('coreautha', '^OK$')) || (await this.has('SecurityAgent', '^OK$')), 8_000);
      if (extra.ok) { const s = await this.answerPasswordSheet(5_000); evidence.push(`turn off asked for the password: ${s.detail}`); }
      const gone = await this.waitFor(async () => { const h = await this.helperInstalled(); return !h.job && !h.file; }, 90_000);
      checks.push({ ok: gone.ok, detail: gone.ok ? 'the launchd job and the file are gone after Turn off' : 'the helper was still installed 90 s after Turn off' });
      const word = await this.waitFor(async () => (await this.texts()).some((t) => /^Off$/.test(t.text)), 30_000);
      checks.push({ ok: word.ok, detail: word.ok ? 'the card reads "Off" again' : 'the card did not read "Off" after Turn off' });
    }
    evidence.push(`screenshot: ${await this.shot('helper-off')}`);
    return this.finish(title, checks, evidence, 'Turn off removed the launchd job and the file');
  }


  /**
   * macOS asks "Allow dyno-agent to find devices on local networks?" the first time the agent touches the local network (Wake-on-LAN,
   * the neighbour table). A person answers Allow; so does the check, and the result says it appeared. Returns how many were answered.
   */
  async answerSystemPrompts() {
    let n = 0;
    const win = await this.g.exec(this.axCmd('windows'));
    const owners = [...new Set(win.stdout.split('\n').map((l) => l.split('\t')[0]).filter((o) => o && !/^(Dyno|Control Center|Dock|Window Server|Finder|Terminal|Notification Center|SystemUIServer)$/.test(o)))];
    for (const who of [...new Set(['UserNotificationCenter', 'CoreServicesUIAgent', ...owners])]) {
      while (n < 5 && (await this.has(who, '^Allow( |$)'))) { await this.shot(`system-prompt-${++n}`); await this.press(who, '^Allow( |$)'); await this.g.sleep(1000); }
    }
    return n;
  }

  async closeSettings() {
    if (await this.has(this.app, '^Settings$', 'AXWindow')) await this.press(this.app, '^Close$', 'AXButton');
  }

  /** Index of the first text field whose label matches `re` among the app's text fields, in the order `axdrive setfield` counts them. */
  async fieldIndex(re) {
    const rows = await this.texts();
    const fields = rows.filter((r) => r.role === 'AXTextField' || r.role === 'AXSecureTextField');
    return fields.findIndex((f) => re.test(f.text));
  }

  async setField(re, value) {
    const n = await this.fieldIndex(re);
    if (n < 0) return false;
    return (await this.g.exec(`printf '%s' ${sq(value)} | ${this.axCmd(`setfield ${sq(this.app)} ${n}`)}`)).code === 0;
  }

  /** The Power page on one of its tabs (the real tab buttons; AXPress on a radio needs the trailing value in its label, so no `$`). */
  async openPower(tab) {
    await this.ensureMainWindow();
    if (!(await this.has(this.app, '^Power$', 'AXHeading'))) await this.press(this.app, '^Power$', 'AXLink');
    const ok = await this.waitFor(() => this.press(this.app, `^${tab}( |$)`, 'AXRadioButton'), 20_000);
    if (ok.ok) await this.waitFor(async () => (await this.texts()).some((t) => t.role === 'AXRadioButton' && t.text.startsWith(tab)), 5_000);
    return ok.ok;
  }

  async headingMatches(re) { return (await this.texts()).some((t) => t.role === 'AXHeading' && re.test(t.text)); }

  /** Wake devices > Add by hand: name and address, then Add computer. */
  async addWakeDevice(name, mac) {
    if (!(await this.openPower('Wake devices'))) return { ok: false, detail: 'no Wake devices tab' };
    if (!(await this.press(this.app, '^Add by hand$'))) return { ok: false, detail: 'no "Add by hand" button' };
    const f = await this.waitFor(() => this.fieldIndex(/^Name of the computer/).then((i) => i >= 0), 10_000);
    if (!f.ok) return { ok: false, detail: 'the Add a computer form did not open' };
    const set = (await this.setField(/^Name of the computer/, name)) && (await this.setField(/^Hardware address/, mac));
    const add = set && (await this.press(this.app, '^Add computer$'));
    const row = add && (await this.waitFor(async () => this.has(this.app, `^Wake ${name}`), 15_000)).ok;
    return { ok: Boolean(row), detail: row ? `saved "${name}" (${mac}) in Wake devices; its row offers "Wake ${name}"` : 'the computer was not saved' };
  }

  /** Schedule > Turn on scheduled wake (when offered) > Add schedule: name, time of day (12-hour text), Once, then Add schedule. */
  async addSchedule(name, time12) {
    if (!(await this.openPower('Schedule'))) return { ok: false, detail: 'no Schedule tab' };
    if (await this.has(this.app, '^Turn on scheduled wake$')) {
      await this.press(this.app, '^Turn on scheduled wake$');
      // An Add press while the page re-renders after "Turn on" is lost (0.2 s after: no form ever; 4 s after: form): wait for the banner to go, a condition, not a delay.
      if (!(await this.waitFor(async () => !(await this.has(this.app, '^Turn on scheduled wake$')), 20_000)).ok) return { ok: false, detail: '"Turn on scheduled wake" was pressed but the banner never went' };
    }
    if (!(await this.waitFor(() => this.press(this.app, '^Add schedule$'), 15_000)).ok) return { ok: false, detail: 'no "Add schedule" button' };
    const open = await this.waitFor(async () => (await this.fieldIndex(/^Name of the schedule/)) >= 0, 10_000);
    if (!open.ok) return { ok: false, detail: 'the Add a schedule form did not open' };
    const set = (await this.setField(/^Name of the schedule/, name)) && (await this.setField(/^Time of day to wake/, time12));
    await this.press(this.app, '^Once', 'AXRadioButton');
    const add = set && (await this.press(this.app, '^Add schedule$'));
    const row = add && (await this.waitFor(async () => (await this.texts()).some((t) => t.text === name && t.role === 'AXStaticText'), 15_000)).ok;
    return { ok: Boolean(row), detail: row ? `added the schedule "${name}" at ${time12}, Once` : 'the schedule was not added' };
  }

  finish(title, checks, evidence, passSummary = '') {
    const failed = checks.filter((c) => !c.ok);
    return { title, status: failed.length ? 'fail' : 'pass', summary: failed.length ? failed.map((c) => c.detail).join('; ').slice(0, 240) : passSummary, checks, evidence };
  }

  /** Raw-text rules of the shared screenshot check, run on the texts the app really shows. `problems` is rawTextProblems. */
  async screenCheck(label, problems) {
    const texts = (await this.texts()).map((t) => ({ text: t.text, where: `${label} (${t.role})` })).filter((t) => t.text.length > 0);
    return { texts: texts.length, problems: problems(texts) };
  }
}
