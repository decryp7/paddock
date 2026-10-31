// The scripted update test of `scripts/vm-smoke --update-test <assets>` (README, "Update test"): the installed OLD build (signed for a
// throw-away test key) finds a NEW build in a local HTTPS feed, downloads it, verifies its signature, installs it and relaunches; and
// refuses a feed whose signature is wrong. Steps do what a person does in the Updates window (Accessibility API), and every wait is a
// poll on state with a guard. Verdicts come from update-judge.mjs. The assets are made by spotter-update-assets.sh (TEST keys only).
import { appWindows } from './gui-judge.mjs';
import { CASES, plistVersion, staticTexts, updateState } from './update-judge.mjs';

const sq = (s) => `'${String(s).replace(/'/g, `'\\''`)}'`;
const CA_IN_GUEST = '/Volumes/My Shared Files/stage/ca.pem';

export class UpdateTest {
  /**
   * @param {import('./gui.mjs').GuiCheck} check  the GUI check (guest, AX helpers, steps)
   * @param {{ setCase(id: string): Promise<void>, feedLog(): Promise<string[]>, clearFeedLog(): Promise<void> }} feed  the host's local HTTPS feed (update-feed.mjs)
   * @param {{ old: string, new: string }} plan
   */
  constructor(check, feed, plan) {
    this.c = check;
    this.feed = feed;
    this.plan = plan;
    this.owner = check.gui.windowOwner;
    this.app = check.config.bundle.replace(/\.app$/, '');
  }

  g() { return this.c.g; }
  guard() { return this.c.guard(); }
  async installedVersion() { return plistVersion((await this.g().exec(`defaults read ${sq(`${this.c.bundle}/Contents/Info`)} CFBundleShortVersionString`)).stdout); }
  async pids() { return (await this.g().exec(`pgrep -f ${sq(`${this.c.bundle}/Contents/MacOS/`)}`)).stdout.split('\n').filter(Boolean).sort().join(','); }
  async texts() { return staticTexts((await this.c.ax(['tree', this.owner, '10'])).stdout); }
  async state() { return updateState(await this.texts(), this.plan.new); }

  /** The test CA (made for this run) is trusted by the guest only, so the app's HTTPS client accepts the local feed. */
  async trustCa() {
    const r = await this.g().exec(`sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain ${sq(CA_IN_GUEST)}`);
    return this.c.step('Test CA trusted in the VM', [{ ok: r.code === 0, detail: r.code === 0 ? 'throw-away CA added to the guest\'s System keychain' : `security: ${r.stderr.trim().slice(0, 160)}` }], { passSummary: 'throw-away CA trusted (this VM only)' });
  }

  /** Help > Check for updates… (the menu item is pressed through the Accessibility API), then the Updates window must exist. */
  async openUpdates() {
    const item = '^Check for updates';
    const pressed = await this.until(async () => (await this.c.press(this.owner, item, 'AXMenuItem')), this.guard());
    return pressed.ok && (await this.until(async () => (await this.texts()).length > 0, this.guard())).ok;
  }

  until(fn, ms) { return this.c.until(fn, ms); }

  /** Presses a button of the Updates window by its label, waiting for it to exist. */
  async button(label) {
    const re = `^${label}( \\||$)`;
    const r = await this.until(async () => (await this.c.has(this.owner, re)) && (await this.c.press(this.owner, re)), this.guard());
    return r.ok;
  }

  async waitState(want, ms = 3 * this.guard()) {
    let last = { state: 'unknown' };
    const r = await this.until(async () => { last = await this.state(); return want.includes(last.state) ? last : null; }, ms);
    return r.ok ? r.value : { ...last, timedOut: true };
  }

  /** One case. `down` stops the feed; the other cases serve their folder. */
  async runCase(cs) {
    const title = cs.title;
    const checks = [];
    const before = await this.installedVersion();
    const startedPids = await this.pids();
    await this.feed.clearFeedLog();
    await this.feed.setCase(cs.id);
    const opened = await this.openUpdates();
    checks.push({ ok: opened, detail: opened ? 'Help > Check for updates… opened the Updates window' : 'the Updates window did not open' });
    if (!opened) { await this.c.shot(`${cs.id}-no-window`); return this.c.step(title, checks); }
    if (!(await this.button('Check (now|again)'))) checks.push({ ok: true, detail: 'no Check button: the window had already started a check' });
    if (cs.id === 'down') {
      const s = await this.waitState(['failed', 'unavailable', 'upToDate', 'available'], this.guard());
      await this.c.shot('down-result');
      checks.push({ ok: s.state === 'failed', detail: `window: ${s.state}${s.detail ? ` (${s.detail})` : ''}` });
      checks.push({ ok: (await this.pids()) === startedPids && startedPids !== '', detail: 'the app is still running, the same process' });
      checks.push({ ok: (await this.installedVersion()) === this.plan.old, detail: `installed version still ${this.plan.old}` });
      return this.c.step(title, checks, { passSummary: 'a failed check says so; the app keeps running' });
    }
    const avail = await this.waitState(['available']);
    checks.push({ ok: avail.state === 'available', detail: `window: ${avail.state} (wanted "Version ${this.plan.new} is available")` });
    if (avail.state !== 'available') { await this.c.shot(`${cs.id}-not-available`); return this.c.step(title, checks); }
    await this.c.shot(`${cs.id}-available`);
    checks.push({ ok: await this.button('Download'), detail: 'pressed Download' });
    const done = await this.waitState(cs.failed ? ['failed'] : ['ready']);
    await this.c.shot(`${cs.id}-after-download`);
    const log = await this.feed.feedLog();
    checks.push({ ok: log.some((l) => /latest\.json/.test(l)), detail: 'the feed saw the manifest request' });
    checks.push({ ok: log.some((l) => /\.app\.tar\.gz( |$)/.test(l)) === cs.tarballRequested, detail: `the feed ${cs.tarballRequested ? 'served' : 'did not serve'} the update file (${log.length} request(s))` });
    if (cs.failed) {
      checks.push({ ok: done.state === 'failed', detail: `window: ${done.state}${done.detail ? ` — ${done.detail}` : ''} (wanted a refusal)` });
      checks.push({ ok: (await this.installedVersion()) === this.plan.old, detail: `installed version still ${this.plan.old}` });
      checks.push({ ok: (await this.pids()) === startedPids, detail: 'the app was not restarted' });
      return this.c.step(title, checks, { passSummary: 'refused; the installed build is untouched' });
    }
    checks.push({ ok: done.state === 'ready', detail: `window: ${done.state} (wanted "downloaded and its signature verified")` });
    if (done.state !== 'ready') return this.c.step(title, checks);
    checks.push({ ok: await this.button('Restart to update'), detail: 'pressed Restart to update' });
    const back = await this.until(async () => {
      const v = await this.installedVersion();
      const p = await this.pids();
      return v === this.plan.new && p !== '' && p !== startedPids && appWindows(await this.c.windows(), this.owner).length > 0 ? { v, p } : null;
    }, 3 * this.guard());
    await this.c.shot(`${cs.id}-relaunched`);
    checks.push({ ok: back.ok, detail: back.ok ? `relaunched on ${this.plan.new} (new process ${back.value.p}, window shown)` : `no relaunch on ${this.plan.new} (installed: ${await this.installedVersion()})` });
    const sign = await this.g().exec(`codesign --verify --deep --strict ${sq(this.c.bundle)}`);
    checks.push({ ok: sign.code === 0, detail: sign.code === 0 ? 'codesign --verify --deep --strict passes' : `codesign: ${sign.stderr.trim().slice(0, 120)}` });
    const count = (await this.pids()).split(',').filter(Boolean).length;
    checks.push({ ok: count === 1, detail: `${count} app process(es) (single instance)` });
    const q = await this.c.quarantine(this.c.bundle);
    checks.push({ ok: true, detail: `quarantine on the updated bundle: ${q?.raw ?? 'none'} (recorded, not judged)` });
    return this.c.step(title, checks, { passSummary: `${before} to ${this.plan.new}: found, downloaded, verified, installed, relaunched` });
  }

  /** The order: the refusals first (on the OLD build, which each leaves in place), the success last. */
  async runAll(push) {
    for (const id of ['down', 'badsig', 'wrongkey', 'ok']) {
      const cs = CASES.find((x) => x.id === id);
      const s = push(await this.runCase(cs));
      if (s.status === 'fail' && id !== 'ok') this.c.log(`case ${id} failed; continuing with the others`);
    }
  }
}

/** The update-test sequence: install the OLD build the way a user does (download, drag, Gatekeeper), then the cases. */
export async function runUpdateTest(check, feed, plan, ut = new UpdateTest(check, feed, plan)) {
  const steps = [];
  const push = (s) => { steps.push(s); check.log(`${s.status.toUpperCase()}  ${s.title}: ${s.summary}`); return s; };
  for (const fn of ['preconditions', 'download', 'installByDrag', 'gatekeeperFirstLaunch']) {
    if (push(await check[fn]()).status === 'fail') { steps.push(check.screenshots()); return { steps, version: check.version }; }
  }
  const v = await ut.installedVersion();
  push(check.step('Installed build is the OLD test build', [{ ok: v === plan.old, detail: `Info.plist says ${v || 'nothing'} (wanted ${plan.old})` }], { passSummary: `${plan.old} installed, quarantined, opened through Gatekeeper` }));
  if (steps.at(-1).status === 'fail') { steps.push(check.screenshots()); return { steps, version: check.version }; }
  push(await ut.trustCa());
  await ut.runAll(push);
  push(await check.core.quit('Quit at the end'));
  push(await check.core.logs());
  steps.push(check.screenshots());
  return { steps, version: check.version };
}
