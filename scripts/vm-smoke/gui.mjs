// The GUI steps of `scripts/vm-smoke --gui` (README: "GUI mode"), against a guest on the GUI test image (gui-image.sh) and a few
// host hooks (the VM's own VNC pointer, the reboot, the served installer). The steps do what a person does: download with Safari,
// drag to Applications, Gatekeeper's first launch, the window, Launch at login with the approval path, a reboot, a Finder
// double-click. Every wait is a poll on state with a guard; the verdicts come from judge.mjs and gui-judge.mjs. Screenshots are
// taken inside the guest. In the guest, `axdrive` (gui/axdrive.swift) drives the Accessibility API.
import { Check } from './checks.mjs';
import { describeTree, instancesOf, parseLaunchctlPrint, parsePs } from './judge.mjs';
import { appWindows, btmItem, judgeLoginItems, judgeLoginStart, parseBtmDump, parseLoginItems, parseQuarantine, parseWindows } from './gui-judge.mjs';

const sq = (s) => `'${String(s).replace(/'/g, `'\\''`)}'`;
/** Background rows the GUI test image has of its own (cirruslabs' guest agent; its limit.maxfiles daemon shows as "launchctl"). */
export const IMAGE_BACKGROUND_ROWS = ['Cirrus Labs, Inc.', 'launchctl'];
/** The image's public default password (cirruslabs images: admin/admin). It is a test VM's, never a real one. */
const VM_PASSWORD = 'admin';
const EV = '/Users/admin/vmsmoke-ev';

/**
 * @typedef {{ exec(cmd: string, opts?: {timeoutMs?: number, input?: string}): Promise<{code:number, stdout:string, stderr:string}>, now(): number, sleep(ms: number): Promise<void> }} Guest
 * @typedef {{ installerUrl: string, installerSha256: string, drag(from:{x:number,y:number}, to:{x:number,y:number}): Promise<void>, doubleClick(at:{x:number,y:number}): Promise<void>, reboot(): Promise<boolean> }} Host
 */
export class GuiCheck {
  /** @param {Guest} guest @param {Host} host */
  constructor(guest, config, host, { log = () => {}, intervalMs = 1000 } = {}) {
    this.g = guest;
    this.config = config;
    this.gui = config.gui;
    this.host = host;
    this.log = log;
    this.core = new Check(guest, config, { log, intervalMs });
    this.bundle = this.core.bundle;
    this.shots = [];
    this.loginOn = false;
  }

  until(fn, guardMs) { return this.core.until(fn, guardMs); }
  ax(args, opts) { return this.g.exec(`axdrive ${args.map(sq).join(' ')}`, opts); }
  async has(app, re, role = 'AXButton') { return (await this.ax(['has', app, re, role])).code === 0; }
  async press(app, re, role = 'AXButton') { return (await this.ax(['press', app, re, role])).code === 0; }
  async value(app, re, role) { const r = await this.ax(['value', app, re, role]); return r.code === 0 ? r.stdout.trim() : null; }
  async center(app, re, role) {
    const m = /^(\d+) (\d+)$/.exec((await this.ax(['center', app, re, role])).stdout.trim());
    return m ? { x: Number(m[1]), y: Number(m[2]) } : null;
  }
  async windows() { return parseWindows((await this.ax(['windows'])).stdout); }
  async shot(name) {
    const file = `${String(this.shots.length + 1).padStart(2, '0')}-${name}.png`;
    await this.g.exec(`mkdir -p ${EV} && screencapture -x ${sq(`${EV}/${file}`)}`);
    this.shots.push(file);
    return file;
  }
  guard() { return (this.gui.guardSec ?? 60) * 1000; }
  step(title, checks, { evidence = [], passSummary, skip } = {}) {
    if (skip) return { title, status: 'skip', summary: skip, checks, evidence };
    const ok = checks.every((c) => c.ok);
    return { title, status: ok ? 'pass' : 'fail', summary: ok ? passSummary : checks.filter((c) => !c.ok).map((c) => c.detail).join('; ').slice(0, 240), checks, evidence };
  }
  async quarantine(path) { return parseQuarantine((await this.g.exec(`xattr -p com.apple.quarantine ${sq(path)} 2>/dev/null`)).stdout); }
  dmgPath() { return `/Users/admin/Downloads/${this.host.installerUrl.split('/').pop()}`; }

  /** The image is the GUI test image, Gatekeeper is on and the driver is trusted: without these no later row means anything. */
  async preconditions() {
    const stamp = await this.g.exec('cat /usr/local/share/paddock-vmgui/image.txt 2>/dev/null');
    const gk = (await this.g.exec('spctl --status 2>&1')).stdout.trim();
    const aqua = (await this.g.exec('launchctl managername 2>&1')).stdout.trim();
    const trusted = await this.ax(['trusted']);
    return this.step('GUI test image ready', [
      // The stamp records which build ran; a guest without axdrive or with Gatekeeper off fails the checks below anyway.
      { ok: true, detail: stamp.code === 0 ? `image: ${stamp.stdout.trim().replace(/\n/g, '; ')}` : 'image has no stamp (built before gui-image.sh; refresh it with gui-image.sh --refresh)' },
      { ok: gk === 'assessments enabled', detail: `Gatekeeper: \`${gk}\`` },
      { ok: aqua === 'Aqua', detail: `commands run in the ${aqua || 'unknown'} session` },
      { ok: trusted.code === 0, detail: trusted.code === 0 ? 'axdrive is trusted for Accessibility' : 'axdrive is not AX-trusted' },
    ], { passSummary: 'GUI image, Gatekeeper on, Aqua session, driver trusted' });
  }

  /** Safari inside the guest downloads the installer from the host (bridge only); Safari writes the quarantine attribute itself. */
  async download() {
    const title = 'Download with Safari (quarantine)';
    const dmg = this.dmgPath();
    await this.g.exec(`rm -f ${sq(dmg)}; open -a Safari ${sq(this.host.installerUrl)}`);
    const got = await this.until(async () => {
      if (await this.has('Safari', '^Allow( \\||$)')) await this.press('Safari', '^Allow( \\||$)'); // "Do you want to allow downloads on …?"
      const r = await this.g.exec(`[ -f ${sq(dmg)} ] && [ ! -e ${sq(`${dmg}.download`)} ] && xattr -p com.apple.quarantine ${sq(dmg)}`);
      return r.code === 0;
    }, 5 * this.guard());
    await this.g.exec(`osascript -e 'tell application "Safari" to quit' >/dev/null 2>&1`);
    const q = await this.quarantine(dmg);
    const sum = (await this.g.exec(`shasum -a 256 ${sq(dmg)} 2>/dev/null | cut -d' ' -f1`)).stdout.trim();
    return this.step(title, [
      { ok: got.ok, detail: got.ok ? `downloaded in ${(got.tookMs / 1000).toFixed(0)} s to ~/Downloads` : 'no finished download within the guard' },
      { ok: q?.agent === 'Safari' && !q.approved, detail: `quarantine \`${q?.raw ?? 'absent'}\` (written by ${q?.agent ?? 'nobody'})` },
      { ok: sum === this.host.installerSha256, detail: `sha256 ${sum ? (sum === this.host.installerSha256 ? 'matches the installer' : `differs: ${sum}`) : 'unreadable'}` },
    ], { passSummary: `Safari download, quarantine ${q?.raw.split(';')[0]}` });
  }

  /**
   * Opens the dmg as a double-click does and drags the app onto the Applications alias with real pointer events through the VM's
   * VNC server (a Finder `duplicate` leaves the copy translocated, and pointer events posted inside the guest start no drag).
   * The window is clicked once first so it is key; the first press after it opens often only selects the icon, so the drag is
   * retried, judged by state (the bundle in /Applications), at most four times.
   */
  async installByDrag() {
    const title = 'Open the dmg and drag to Applications';
    const app = this.config.bundle.replace(/\.app$/, '');
    await this.g.exec(`sudo rm -rf ${sq(this.bundle)}; open ${sq(this.dmgPath())}`);
    const icons = await this.until(async () => {
      const from = await this.center('Finder', `^${app} \\| ${app}( |$)`, 'AXImage');
      const to = await this.center('Finder', '^Applications \\| Applications', 'AXImage');
      return from && to ? { from, to } : null;
    }, 3 * this.guard());
    if (!icons.ok) return this.step(title, [{ ok: false, detail: 'the dmg window with the app and the Applications alias did not appear' }]);
    await this.shot('dmg-window');
    let attempts = 0;
    let copied = false;
    while (!copied && attempts < 4) {
      attempts++;
      await this.host.drag(icons.value.from, icons.value.to);
      copied = (await this.until(async () => (await this.g.exec(`[ -x ${sq(`${this.bundle}/Contents/MacOS`)} ]`)).code === 0, 20_000)).ok;
    }
    if (copied) await this.until(async () => (await this.g.exec(`codesign --verify --deep --strict ${sq(this.bundle)}`, { timeoutMs: 300_000 })).code === 0, 3 * this.guard());
    const q = await this.quarantine(this.bundle);
    this.version = (await this.g.exec(`defaults read ${sq(`${this.bundle}/Contents/Info`)} CFBundleShortVersionString`)).stdout.trim();
    return this.step(title, [
      { ok: copied, detail: copied ? `copied by a pointer drag (attempt ${attempts})` : `no ${this.bundle} after ${attempts} drags` },
      { ok: !!q && !q.approved, detail: `the copy carries quarantine \`${q?.raw ?? 'absent'}\`` },
    ], { passSummary: `${this.config.bundle} ${this.version} in /Applications by drag, quarantined` });
  }

  /** Gatekeeper's first launch on macOS 15/26: Not Opened > Done > Privacy & Security > Open Anyway > Open? > Open Anyway > password. */
  async gatekeeperFirstLaunch() {
    const title = 'Gatekeeper first launch (Open Anyway)';
    const name = this.gui.windowOwner;
    const checks = [];
    const fail = async (detail) => { checks.push({ ok: false, detail }); await this.shot('gatekeeper-fail'); return this.step(title, checks); };
    const texts = async (app) => (await this.ax(['tree', app, '4'])).stdout.split('\n').filter((l) => /AXStaticText {2}/.test(l)).map((l) => l.replace(/^\s*AXStaticText {2}/, '').replace(/ \| _NS:\d+$/, '')).filter(Boolean);
    await this.g.exec(`osascript -e ${sq(`tell application "Finder" to open (POSIX file "${this.bundle}" as alias)`)}`);
    if (!(await this.until(() => this.has('CoreServicesUIAgent', 'Not Opened', 'AXStaticText'), this.guard())).ok) return fail('no "Not Opened" dialog: Gatekeeper did not stop the first launch');
    await this.shot('gatekeeper-not-opened');
    checks.push({ ok: true, detail: `dialog: ${(await texts('CoreServicesUIAgent')).join(' / ')}` });
    await this.press('CoreServicesUIAgent', '^Done( \\||$)');
    await this.g.exec('open "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension"');
    if (!(await this.until(() => this.has('System Settings', '^Open Anyway'), this.guard())).ok) return fail('no Open Anyway in Privacy & Security');
    await this.shot('gatekeeper-privacy-security');
    checks.push({ ok: true, detail: 'Privacy & Security offers Open Anyway' });
    await this.press('System Settings', '^Open Anyway');
    if (!(await this.until(() => this.has('CoreServicesUIAgent', '^Open Anyway( \\||$)'), this.guard())).ok) return fail('no "Open?" confirmation after Open Anyway');
    await this.shot('gatekeeper-confirm');
    checks.push({ ok: true, detail: `dialog: ${(await texts('CoreServicesUIAgent'))[0] ?? ''}` });
    await this.press('CoreServicesUIAgent', '^Open Anyway( \\||$)');
    if (!(await this.until(() => this.has('coreautha', '^OK$'), this.guard())).ok) return fail('no administrator password sheet');
    await this.shot('gatekeeper-password');
    await this.ax(['setfield', 'coreautha', '1'], { input: `${VM_PASSWORD}\n` });
    await this.press('coreautha', '^OK$');
    const gone = await this.until(async () => !(await this.has('coreautha', '^OK$')), this.guard());
    checks.push({ ok: gone.ok, detail: gone.ok ? 'administrator password accepted' : 'the password sheet stayed' });
    const win = await this.windowWithContent();
    checks.push(...win.checks);
    const procs = parsePs((await this.g.exec('ps -axww -o pid=,ppid=,etime=,lstart=,command=')).stdout);
    const translocated = procs.filter((p) => /AppTranslocation\//.test(p.command) && p.command.includes(`/${this.config.bundle}/`));
    checks.push({ ok: translocated.length === 0, detail: translocated.length ? `runs translocated: ${translocated[0].command}` : `runs from ${this.bundle}` });
    const q = await this.quarantine(this.bundle);
    checks.push({ ok: !!q?.approved, detail: `quarantine after the launch \`${q?.raw ?? 'absent'}\` (0x40 = user approved)` });
    return this.step(title, checks, { passSummary: 'blocked, Open Anyway, confirmed, password, window with content, not translocated' });
  }

  /** A window of the app (CGWindowList) whose AX tree shows the content the config names, not just a frame. */
  async windowWithContent() {
    const owner = this.gui.windowOwner;
    const w = await this.until(async () => appWindows(await this.windows(), owner).length > 0, 2 * this.guard());
    const c = await this.until(() => this.has(owner, this.gui.contentRegex, 'AXStaticText'), 2 * this.guard());
    const wins = appWindows(await this.windows(), owner);
    await this.shot('window');
    return this.step('Window with content', [
      { ok: w.ok, detail: w.ok ? `window: ${wins.map((x) => `"${x.title}" ${x.w}x${x.h}`).join(', ')}` : `no window of ${owner} within the guard` },
      { ok: c.ok, detail: c.ok ? `content /${this.gui.contentRegex}/ rendered` : `no text matching /${this.gui.contentRegex}/ in the window` },
    ], { passSummary: 'window shown with its content' });
  }

  /** Runs one of the config's AX actions, ["press", app, regex, role?], once the element is there (guarded). */
  async act(a) {
    if (a[0] !== 'press') throw new Error(`unknown gui action ${a[0]}`);
    const role = a[3] ?? 'AXButton';
    return (await this.until(() => this.has(a[1], a[2], role), this.guard())).ok && this.press(a[1], a[2], role);
  }

  /** Switches the app's own Launch at login setting through its UI, the way the config describes (open, switch, save). */
  async setLaunchAtLogin(on) {
    const l = this.gui.launchAtLogin;
    if (!(await this.has(...l.switch))) {
      for (const a of l.open) if (!(await this.act(a))) return false;
      if (!(await this.until(() => this.has(...l.switch), this.guard())).ok) return false;
    }
    if ((await this.value(...l.switch)) !== (on ? '1' : '0')) await this.press(...l.switch);
    if (l.save && (await this.has(...l.save))) await this.press(...l.save);
    return (await this.until(async () => (await this.value(...l.switch)) === (on ? '1' : '0'), this.guard())).ok;
  }

  async btm() { return btmItem(parseBtmDump((await this.g.exec('sudo sfltool dumpbtm 2>/dev/null', { timeoutMs: 60_000 })).stdout), this.gui.launchAtLogin.label); }
  async job() { return parseLaunchctlPrint((await this.g.exec(`launchctl print gui/$(id -u)/${this.gui.launchAtLogin.label} 2>&1`)).stdout); }
  async loginItemsRows() {
    await this.g.exec('open "x-apple.systempreferences:com.apple.LoginItems-Settings.extension"');
    await this.until(() => this.has('System Settings', '^background-switch-', 'AXCheckBox'), this.guard());
    return parseLoginItems((await this.ax(['tree', 'System Settings', '40'])).stdout);
  }
  async closeSettings() {
    await this.g.exec(`osascript -e 'tell application "System Settings" to quit' >/dev/null 2>&1`);
    await this.until(async () => (await this.g.exec('pgrep -x "System Settings"')).code !== 0, this.guard());
  }

  /**
   * Sets a Login Items switch and judges it by its own AX value. The pane rebuilds its list right after it opens; a press then is
   * lost (measured once in four runs), so it presses again, at most three times.
   */
  async setLoginItemsSwitch(sw, on) {
    const want = on ? '1' : '0';
    let presses = 0;
    while (presses < 3) {
      if ((await this.value('System Settings', sw, 'AXCheckBox')) === want) return { ok: true, presses };
      presses++;
      await this.press('System Settings', sw, 'AXCheckBox');
      if ((await this.until(async () => (await this.value('System Settings', sw, 'AXCheckBox')) === want, 5_000)).ok) return { ok: true, presses };
    }
    return { ok: false, presses };
  }

  /** Launch at login on: launchd has the job, BTM says enabled+allowed, and Login Items shows exactly the app's rows. */
  async launchAtLoginOn() {
    const title = 'Launch at login on: Login Items rows';
    const l = this.gui.launchAtLogin;
    if (!l) return this.step(title, [], { skip: 'the app has no Launch at login setting (config gui.launchAtLogin is null)' });
    const set = await this.setLaunchAtLogin(true);
    this.loginOn = set;
    const loaded = await this.until(async () => (await this.job()).state !== null, this.guard());
    const item = await this.btm();
    const rows = await this.loginItemsRows();
    await this.shot('login-items-on');
    await this.closeSettings();
    const j = judgeLoginItems(rows, l.rows, IMAGE_BACKGROUND_ROWS);
    return this.step(title, [
      { ok: set, detail: set ? 'switched on in the app\'s Settings' : 'the app\'s switch did not turn on' },
      { ok: loaded.ok, detail: loaded.ok ? `launchd has ${l.label} (state ${(await this.job()).state})` : `launchd has no ${l.label}` },
      { ok: !!item?.enabled && !!item?.allowed, detail: `BTM ${l.label}: ${item?.disposition ?? 'no record'}` },
      ...j.checks,
    ], { passSummary: `registered; Login Items rows as expected (${[...(l.rows.openAtLogin ?? []), ...(l.rows.background ?? [])].join(', ')})` });
  }

  /**
   * The approval path: a user who once switched the item off in Login Items switches Launch at login on again in the app; macOS
   * then needs approval (SMAppService requiresApproval), the app says so and opens Login Items, and the person turns it on there.
   * The pane rebuilds its list after the app opens it; a press then is lost, so it is judged by the switch's value (at most 3).
   */
  async approvalPath() {
    const title = 'Login Items approval (requiresApproval path)';
    const l = this.gui.launchAtLogin;
    if (!l) return this.step(title, [], { skip: 'the app has no Launch at login setting' });
    const sw = `^background-switch-${l.rows.background[0]} \\|`;
    const checks = [];
    await this.loginItemsRows();
    const off = await this.setLoginItemsSwitch(sw, false);
    checks.push({ ok: off.ok, detail: `switched off in Login Items (as a user who refused it once, ${off.presses} press(es)): BTM ${(await this.btm())?.disposition ?? 'no record'}` });
    await this.closeSettings();
    await this.setLaunchAtLogin(false);
    await this.setLaunchAtLogin(true);
    const opened = await this.until(() => this.has('System Settings', sw, 'AXCheckBox'), this.guard());
    const said = (await this.ax(['tree', this.gui.windowOwner, '40'])).stdout.match(new RegExp(l.needsApprovalText))?.[0];
    await this.shot('needs-approval');
    checks.push({ ok: opened.ok, detail: opened.ok ? 'the app opened Login Items for the approval' : 'Login Items did not open' });
    checks.push({ ok: !!said, detail: said ? `the app says: "${said}"` : `the app shows no /${l.needsApprovalText}/` });
    const { ok: on, presses } = await this.setLoginItemsSwitch(sw, true);
    const allowed = await this.until(async () => { const i = await this.btm(); return i?.enabled && i?.allowed; }, this.guard());
    const loaded = await this.until(async () => (await this.job()).state !== null, this.guard());
    await this.shot('approved');
    await this.closeSettings();
    this.loginOn = allowed.ok;
    checks.push({ ok: on, detail: on ? `switched on in Login Items (${presses} press(es))` : 'the Login Items switch did not turn on' },
      { ok: allowed.ok, detail: `BTM after approval: ${(await this.btm())?.disposition ?? 'no record'}` },
      { ok: loaded.ok, detail: loaded.ok ? `launchd has ${l.label}` : `launchd has no ${l.label} after the approval` });
    return this.step(title, checks, { passSummary: 'refused, needs approval shown, approved in Login Items, enabled and allowed' });
  }

  /** Reboots the guest and returns what the app did at the login: the log since the reboot, windows, the settle window. */
  async rebootAndObserve() {
    const log = await this.core.path(this.gui.loginStart.log);
    const size = Number((await this.g.exec(`stat -f %z ${sq(log)} 2>/dev/null || echo 0`)).stdout.trim()) || 0;
    if (!(await this.host.reboot())) return null;
    this.core.home = null;
    const appSpec = this.config.processes.find((s) => s.role === 'app');
    const helper = this.config.processes.find((s) => s.role === 'helper');
    const up = await this.until(async () => {
      const procs = await this.core.snapshot();
      return instancesOf(procs, appSpec, this.bundle).length > 0 && (!helper || instancesOf(procs, helper, this.bundle).length > 0);
    }, 3 * this.guard());
    const w = await this.core.observe({ running: true, loginService: this.loginOn });
    // A log that rotated over the reboot is read whole: the offset only skips what was written before it.
    const now = Number((await this.g.exec(`stat -f %z ${sq(log)} 2>/dev/null || echo 0`)).stdout.trim()) || 0;
    const since = (await this.g.exec(`tail -c +${now >= size ? size + 1 : 1} ${sq(log)} 2>/dev/null`)).stdout;
    return { up, w, since, windows: await this.windows(), procs: w.snaps.at(-1) };
  }

  /**
   * Reboot as a login item: the app is quit first (a running app is RESTORED by loginwindow, a different open event without the
   * login-item marker; that case is the next step), so the login start comes from the login item alone. The app must start hidden
   * (its log line, no window) and its helper must run once with the parent the config expects, over the settle window.
   */
  async rebootLogin() {
    const title = 'Reboot as a login item (app quit first): hidden, one agent';
    const l = this.gui.launchAtLogin;
    if (!l || !this.loginOn) return this.step(title, [], { skip: 'Launch at login is not on (an earlier step failed or the app has none)' });
    await this.core.quit('(quit before the reboot)');
    const r = await this.rebootAndObserve();
    if (!r) return this.step(title, [{ ok: false, detail: 'the guest did not come back with an Aqua session after the reboot' }]);
    const start = judgeLoginStart(r.since, r.windows, { ...this.gui.loginStart, windowOwner: this.gui.windowOwner });
    await this.shot('after-login-item-start');
    const checks = [{ ok: r.up.ok, detail: r.up.ok ? `the app and its helper ran ${(r.up.tookMs / 1000).toFixed(0)} s after the guest answered` : 'the app did not start at login' }, ...start.checks, ...r.w.checks];
    const appSpec = this.config.processes.find((s) => s.role === 'app');
    const helper = this.config.processes.find((s) => s.role === 'helper');
    if (helper && l.helperParentAfterLogin) {
      const hs = instancesOf(r.procs, helper, this.bundle);
      const want = l.helperParentAfterLogin === 'launchd' ? (p) => p.ppid === 1 : (p) => instancesOf(r.procs, appSpec, this.bundle).some((a) => a.pid === p.ppid);
      checks.push({ ok: hs.length === 1 && hs.every(want), detail: `${helper.name} after the login: ${hs.map((p) => `pid ${p.pid} parent ${p.ppid}`).join(', ') || 'none'} (expected one, parent ${l.helperParentAfterLogin})` });
    }
    const job = await this.job();
    return this.step(title, checks, { evidence: [`launchd ${l.label}: state=${job.state} pid=${job.pid}`, ...describeTree(r.procs, this.config, this.bundle), '--- app log since the reboot', ...r.since.trimEnd().split('\n').slice(-30)], passSummary: 'started hidden by the login item, one helper with the expected parent' });
  }

  /**
   * Reboot with the app running, as people restart a Mac with a tray app in it: loginwindow restores it ("Reopen windows"). How
   * the app should start then is a product decision, `gui.loginStart.restore`: "hidden", "window", or null (report only).
   */
  async rebootRestore() {
    const title = 'Reboot with the app running (restored at login)';
    const want = this.gui.loginStart?.restore;
    if (want === undefined) return this.step(title, [], { skip: 'no gui.loginStart for this app' });
    const appSpec = this.config.processes.find((s) => s.role === 'app');
    if (!instancesOf(await this.core.snapshot(), appSpec, this.bundle).length) await this.core.start();
    await this.until(async () => instancesOf(await this.core.snapshot(), appSpec, this.bundle).length > 0, this.guard());
    const r = await this.rebootAndObserve();
    if (!r) return this.step(title, [{ ok: false, detail: 'the guest did not come back with an Aqua session after the reboot' }]);
    const hidden = new RegExp(this.gui.loginStart.hidden).exec(r.since)?.[0];
    const shown = new RegExp(this.gui.loginStart.shown).exec(r.since)?.[0];
    const wins = appWindows(r.windows, this.gui.windowOwner);
    const got = wins.length || shown ? 'window' : hidden ? 'hidden' : 'unknown';
    await this.shot('after-restore');
    const facts = [{ ok: r.up.ok, detail: r.up.ok ? 'the app was restored at login' : 'the app did not come back after the reboot' },
      { ok: true, detail: `log since the reboot: ${[hidden, shown].filter(Boolean).map((x) => `"${x}"`).join(', ') || 'no start decision logged'}; ${wins.length} window(s)` }, ...r.w.checks];
    const evidence = [...describeTree(r.procs, this.config, this.bundle), '--- app log since the reboot', ...r.since.trimEnd().split('\n').slice(-30)];
    if (want === null) {
      const s = this.step(title, facts, { evidence, passSummary: '' });
      return s.status === 'fail' ? s : { ...s, status: 'info', summary: `restored with: ${got} (expected behaviour not decided yet: gui.loginStart.restore is null)` };
    }
    return this.step(title, [...facts, { ok: got === want, detail: `restored with: ${got} (expected ${want})` }], { evidence, passSummary: `restored ${want}, one of each` });
  }

  /**
   * A Finder double-click on the app must show its window. The start state is the one a person meets: the app running without a
   * window (left so by the login start, or else started with the family's tray start `--hidden` when the config says the app has
   * one, gui.hiddenStart), or not running at all for an app without a tray start.
   */
  async finderDoubleClick() {
    const title = 'Finder double-click opens a window';
    const owner = this.gui.windowOwner;
    const appSpec = this.config.processes.find((s) => s.role === 'app');
    const running = async () => instancesOf(await this.core.snapshot(), appSpec, this.bundle).length > 0;
    const shown = async () => appWindows(await this.windows(), owner).length > 0;
    let from = 'running without a window after the login start';
    if (!(await running()) || (await shown())) {
      if (await running()) await this.core.quit('(quit before the double-click)');
      if (this.gui.hiddenStart) {
        await this.core.start({ hidden: true });
        const up = await this.until(async () => (await running()) && !(await shown()), 2 * this.guard());
        if (!up.ok) return this.step(title, [{ ok: false, detail: 'the --hidden start did not leave the app running without a window' }]);
        from = 'running without a window after `open --args --hidden`';
      } else from = 'not running';
    }
    const name = this.config.bundle.replace(/\.app$/, '');
    await this.g.exec('open /Applications');
    const icon = await this.until(() => this.center('Finder', `^${name}( \\||$)`, 'AXImage'), this.guard());
    if (!icon.ok) return this.step(title, [{ ok: false, detail: `no ${name} icon in the Applications window` }]);
    await this.host.doubleClick(icon.value);
    const win = await this.windowWithContent();
    const one = instancesOf(await this.core.snapshot(), appSpec, this.bundle).length === 1;
    await this.g.exec(`osascript -e 'tell application "Finder" to close every window' >/dev/null 2>&1`);
    return this.step(title, [{ ok: true, detail: `before: ${from}` }, ...win.checks, { ok: one, detail: one ? 'one app process' : 'a second app process' }], { passSummary: `from "${from}", the window with its content` });
  }

  /**
   * Launch at login off again through the app: the launchd job goes, and nothing of the app runs from launchd any more (a Quit
   * with the login item on rightly leaves launchd's agent running, so the final Quit comes after this).
   */
  async launchAtLoginOff() {
    const title = 'Launch at login off';
    const l = this.gui.launchAtLogin;
    if (!l || !this.loginOn) return this.step(title, [], { skip: 'Launch at login is not on' });
    const appSpec = this.config.processes.find((s) => s.role === 'app');
    if (!instancesOf(await this.core.snapshot(), appSpec, this.bundle).length) await this.core.start();
    const off = await this.setLaunchAtLogin(false);
    const gone = await this.until(async () => (await this.job()).state === null, this.guard());
    const w = await this.core.observe({ running: true, loginService: false });
    this.loginOn = !off;
    return this.step(title, [
      { ok: off, detail: off ? 'switched off in the app\'s Settings' : 'the switch did not turn off' },
      { ok: gone.ok, detail: gone.ok ? `launchd no longer has ${l.label}` : `launchd still has ${l.label}` },
      ...w.checks,
    ], { evidence: describeTree(w.snaps.at(-1), this.config, this.bundle), passSummary: 'job removed; the helper is the app\'s child again, one of each' });
  }

  /** Copies of the guest's screenshots are taken by the caller; this lists them as evidence. */
  screenshots() { return { title: 'Screenshots (taken inside the VM)', status: 'info', summary: `${this.shots.length} file(s)`, evidence: this.shots }; }
}

/** The --gui sequence. Stops early when a step that the rest depends on fails. */
export async function runGui(check, { manual = [] } = {}) {
  const steps = [];
  const push = (s) => { steps.push(s); check.log(`${s.status.toUpperCase()}  ${s.title}: ${s.summary}`); return s; };
  for (const fn of ['preconditions', 'download', 'installByDrag', 'gatekeeperFirstLaunch']) {
    if (push(await check[fn]()).status === 'fail') { steps.push(check.screenshots()); return { steps, version: check.version }; }
  }
  push(await check.launchAtLoginOn());
  push(await check.approvalPath());
  push(await check.rebootLogin());
  push(await check.rebootRestore());
  push(await check.finderDoubleClick());
  push(await check.launchAtLoginOff());
  push(await check.core.quit('Quit at the end'));
  push(await check.core.logs());
  steps.push(check.screenshots());
  for (const m of manual) steps.push({ title: m.title, status: 'manual', summary: m.summary });
  return { steps, version: check.version };
}

