// The per-OS drivers of row 5a (login-jobs-row.mjs). Each reads and presses "Start Dyno at login" in Settings and reads the Jobs list through the OS's own
// accessibility API. The shell's Jobs list is a `ul.pd-jobs` (aria-label from the shell) whose rows carry the job name, its status and a Cancel button
// (ui/projects/paddock-ui/shell/jobs/src/jobs-list.ts). Names are matched loosely (/Jobs/, /^Cancel/) because they are the shell's, not Dyno's.
import { loginJobsRow } from './login-jobs-row.mjs';

const SWITCH = /Start Dyno at login/i;
// The shell's status item: "Jobs" when idle, "1 running: <job>" while something runs (paddock-ui jobs-status-text.ts).
const STATUS = '^(Jobs|\\d+ running.*)$';
// A job that lasts long enough to cancel: Benchmarks > "Sustained processor" (every core for minutes, no disk write, no consent), Run benchmarks. The page is left
// for Processes so that only the Jobs list carries a Cancel button.

/** Windows: Playwright over the WebView2 DevTools port. `openSettings(browser)` returns the Settings page. */
export function windowsLoginJobsDriver(browser, { openSettings, mainPage }) {
  const settings = async () => openSettings(browser);
  const box = async () => { const s = await settings(); return s ? s.getByRole('switch', { name: SWITCH }).or(s.getByRole('checkbox', { name: SWITCH })).first() : null; };
  return {
    async readSwitch() { const b = await box(); if (!b || !(await b.count().catch(() => 0))) return 'unknown'; const v = await b.getAttribute('aria-checked').catch(() => null); return v === 'true' ? 'on' : v === 'false' ? 'off' : 'unknown'; },
    async pressSwitch() { const b = await box(); if (!b) return false; return b.click().then(() => true, () => false); },
    // The switch edits a draft; "Save changes" applies it (settings-page.ts). save() only presses it; apply() also waits for "Applying…" to end and reads the SAVED value
    // from a reloaded Settings page (the draft is dropped by the reload).
    async save() { const s = await settings(); if (!s) return false; return s.getByRole('button', { name: /^Save changes$/ }).first().click({ timeout: 5000 }).then(() => true, () => false); },
    async apply() {
      const s = await settings(); if (!s) return { ok: false, detail: 'Settings did not open' };
      const pressed = await s.getByRole('button', { name: /^Save changes$/ }).first().click({ timeout: 5000 }).then(() => true, () => false);
      if (!pressed) return { ok: false, detail: 'no "Save changes" button (nothing to save, or the button is disabled)' };
      for (let i = 0; i < 60; i++) { await new Promise((r) => setTimeout(r, 500)); if (!(await s.getByText(/Applying…/).count().catch(() => 0)) && !(await s.getByRole('button', { name: /^Save changes$/ }).count().catch(() => 0))) break; }
      const err = await s.locator('.save-error').first().innerText({ timeout: 500 }).catch(() => '');
      await s.reload().catch(() => {});
      const b = await box(); await b?.waitFor({ timeout: 15_000 }).catch(() => {});
      const v = b ? await b.getAttribute('aria-checked').catch(() => null) : null;
      return { ok: !err, saved: v === 'true' ? 'on' : v === 'false' ? 'off' : 'unknown', detail: err ? `the save failed: ${err}` : 'saved' };
    },
    async openJobs() {
      const p = mainPage(browser); if (!p) return false;
      await p.bringToFront().catch(() => {});
      if (await p.locator('ul.pd-jobs').count()) return true;
      // The status item's name changes with what runs ("Jobs", "1 running: ..."): find it by its element, not by its words.
      const item = p.locator('pd-jobs-status button').first();
      if (await item.count()) await item.click().catch(() => {});
      for (let i = 0; i < 10 && !(await p.locator('ul.pd-jobs').count()); i++) await new Promise((r) => setTimeout(r, 300));
      return (await p.locator('ul.pd-jobs').count()) > 0;
    },
    async startLongJob() {
      const p = mainPage(browser); if (!p) return false;
      await p.keyboard.press('Escape').catch(() => {});
      await p.getByRole('link', { name: /^Benchmarks$/ }).first().click().catch(() => {});
      const preset = p.getByRole('radio', { name: /^Sustained processor/ }).or(p.getByRole('button', { name: /^Sustained processor/ })).first();
      if (!(await preset.waitFor({ timeout: 15_000 }).then(() => true, () => false))) return false;
      await preset.click().catch(() => {});
      const card = p.locator('section[aria-label="Run benchmarks"]').first();
      const cardText = async () => (await card.innerText({ timeout: 2000 }).catch(() => '')).replace(/\s+/g, ' ').slice(0, 400);
      const run = p.getByRole('button', { name: /Run benchmarks$/ }).first();
      if (!(await run.isEnabled().catch(() => false))) return { ok: false, detail: `Run benchmarks is not enabled; the Run card says: ${await cardText()}` };
      await run.click();
      // Started when the card shows its own Cancel (a run in progress); only then is the page left, so that only the Jobs list carries a Cancel.
      let started = false;
      for (let i = 0; i < 20 && !started; i++) { await new Promise((r) => setTimeout(r, 500)); started = (await card.getByRole('button', { name: /Cancel$/ }).count().catch(() => 0)) > 0; }
      const text = await cardText();
      if (!started) return { ok: false, detail: `the benchmark did not start within 10 s of Run benchmarks; the Run card says: ${text}` };
      await p.getByRole('link', { name: /^Processes$/ }).first().click().catch(() => {});
      return { ok: true, detail: `benchmark running: ${text}` };
    },
    async stopLongJob() {
      const p = mainPage(browser); if (!p) return;
      await p.getByRole('link', { name: /^Benchmarks$/ }).first().click().catch(() => {});
      await p.locator('section[aria-label="Run benchmarks"] button', { hasText: /^\s*\S*\s*Cancel\s*$/ }).first().click({ timeout: 3000 }).catch(() => {});
    },
    async jobsText() { const p = mainPage(browser); return p ? (await p.locator('ul.pd-jobs').first().innerText().catch(() => '')) : ''; },
    async pressCancel() { const p = mainPage(browser); if (!p) return false; return p.locator('ul.pd-jobs').getByRole('button', { name: /^Cancel/ }).first().click({ timeout: 3000 }).then(() => true, () => false); },
  };
}

/** macOS: axdrive over a MacGui. */
export function macLoginJobsDriver(gui) {
  const find = async () => (await gui.texts()).find((t) => SWITCH.test(t.text) && ['AXCheckBox', 'AXSwitch', 'AXButton', 'AXRadioButton', 'AXToggle'].includes(t.role));
  return {
    async readSwitch() { await gui.openSettingsSection('^Dyno$'); const c = await find(); return !c ? 'unknown' : c.value === '1' ? 'on' : c.value === '0' ? 'off' : 'unknown'; },
    async pressSwitch() { const c = await find(); return c ? gui.press(gui.app, 'Start Dyno at login', c.role) : false; },
    async save() { return gui.press(gui.app, '^Save changes$', 'AXButton'); },
    async apply() {
      if (!(await gui.press(gui.app, '^Save changes$', 'AXButton'))) return { ok: false, detail: 'no "Save changes" button' };
      await gui.waitFor(async () => !(await gui.texts()).some((t) => /Applying…|^Save changes$/.test(t.text)), 30_000);
      const err = (await gui.texts()).find((t) => /could not|failed/i.test(t.text) && t.role === 'AXStaticText');
      await gui.closeSettings(); await gui.openSettingsSection('^Dyno$');
      const c = await find();
      return { ok: !err, saved: !c ? 'unknown' : c.value === '1' ? 'on' : c.value === '0' ? 'off' : 'unknown', detail: err ? `the save failed: ${err.text}` : 'saved' };
    },
    async openJobs() { await gui.closeSettings?.(); await gui.ensureMainWindow(); return (await gui.has(gui.app, STATUS, 'AXButton')) ? gui.press(gui.app, STATUS, 'AXButton') : (await gui.texts()).some((t) => /Jobs/i.test(t.text)); },
    async startLongJob() {
      await gui.closeSettings?.(); await gui.ensureMainWindow();
      if (!(await gui.waitFor(() => gui.press(gui.app, '^Benchmarks$', 'AXLink'), 20_000)).ok) return false;
      if (!(await gui.waitFor(() => gui.press(gui.app, '^Sustained processor', 'AXRadioButton'), 15_000)).ok) return false;
      if (!(await gui.press(gui.app, '^Run benchmarks$'))) return false;
      await gui.waitFor(() => gui.press(gui.app, '^Processes$', 'AXLink'), 10_000);
      return true;
    },
    async stopLongJob() { await gui.press(gui.app, '^Benchmarks$', 'AXLink'); await gui.press(gui.app, '^Cancel$', 'AXButton'); },
    async jobsText() { return (await gui.texts()).map((t) => t.text).filter((x) => /Start Dyno at login|Cancell?ed|Working|Cancel$/i.test(x)).join(' | '); },
    async pressCancel() { return gui.press(gui.app, '^Cancel', 'AXButton'); },
  };
}

/** Linux: the AT-SPI helper (`atspi.py`) through `atspi(args)` -> { code, stdout }. */
export function linuxLoginJobsDriver(atspi, parseSwitchState) {
  const SW = '"Start Dyno at login" "check box|toggle button|switch"';
  return {
    async readSwitch() { const r = atspi(`state dyno ${SW}`); return r.code === 0 ? parseSwitchState(r.stdout) : 'unknown'; },
    async pressSwitch() { return atspi(`press dyno ${SW}`).code === 0; },
    async save() { return atspi('press dyno "^Save changes$" "push button"').code === 0; },
    async apply() {
      if (atspi('press dyno "^Save changes$" "push button"').code !== 0) return { ok: false, detail: 'no "Save changes" button' };
      for (let i = 0; i < 15 && /Applying…|Save changes/.test(atspi('tree dyno 60').stdout); i++) await new Promise((r) => setTimeout(r, 2000));
      // Reopen Settings (File > Settings…, Dyno) so the switch shows the saved value, not the draft.
      atspi('press dyno "^Close$" "push button"'); atspi('press dyno "^File$" "menu item"'); await new Promise((r) => setTimeout(r, 1000));
      atspi('press dyno "^Settings…$" "menu item"'); await new Promise((r) => setTimeout(r, 4000)); atspi('press dyno "^Dyno$" "push button"'); await new Promise((r) => setTimeout(r, 1500));
      const r = atspi(`state dyno ${SW}`); const v = r.code === 0 ? parseSwitchState(r.stdout) : 'unknown';
      return { ok: true, saved: v, detail: 'saved' };
    },
    async openJobs() { atspi('press dyno "^Close$" "push button"'); return atspi(`press dyno ${JSON.stringify(STATUS)} "push button|toggle button"`).code === 0 || /Jobs/.test(atspi('tree dyno 60').stdout); },
    async startLongJob() {
      atspi('press dyno "^Close$" "push button"');
      if (atspi('press dyno "^Benchmarks$" "link"').code !== 0) return false;
      let ok = false;
      for (let i = 0; i < 10 && !ok; i++) { ok = atspi('press dyno "^Sustained processor" "radio button|toggle button|push button"').code === 0; if (!ok) atspi('has dyno "^Benchmarks$"'); }
      if (!ok) return false;
      if (atspi('press dyno "Run benchmarks$" "push button"').code !== 0) return false;
      atspi('press dyno "^Processes$" "link"');
      return true;
    },
    async stopLongJob() { atspi('press dyno "^Benchmarks$" "link"'); atspi('press dyno "^Cancel$" "push button"'); },
    async jobsText() { return atspi('tree dyno 80').stdout.split('\n').filter((l) => /Start Dyno at login|Cancell?ed|Working|Cancel/i.test(l)).map((l) => l.trim()).join(' | '); },
    async pressCancel() { return atspi('press dyno "^Cancel" "push button"').code === 0; },
  };
}

export { loginJobsRow };
