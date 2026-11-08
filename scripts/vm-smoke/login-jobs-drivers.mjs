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
      if (!started) {
        // Why not: any dialog or alert the click opened, and the END of the card (its status lines), not only its start.
        const why = await p.evaluate(() => {
          const t = (e) => (e?.innerText ?? '').replace(/\s+/g, ' ').trim();
          const dialogs = [...document.querySelectorAll('[role=dialog],[role=alertdialog],[role=alert],[role=status]')].map(t).filter(Boolean).join(' | ');
          const card = t(document.querySelector('section[aria-label="Run benchmarks"]'));
          const run = [...document.querySelectorAll('button')].find((b) => /Run benchmarks/.test(b.textContent));
          return `dialogs/alerts: ${dialogs.slice(0, 300) || 'none'}; Run button disabled=${run ? run.disabled : 'absent'}; card end: ${card.slice(-300)}`;
        }).catch((e) => `page unreadable: ${e.message}`);
        return { ok: false, detail: `the benchmark did not start within 10 s of Run benchmarks; ${why}` };
      }
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
    // The Jobs step closes Settings (openJobs): every Settings action opens the Dyno section first when the switch is not on screen.
    async pressSwitch() { let c = await find(); if (!c) { await gui.openSettingsSection('^Dyno$'); c = await find(); } return c ? gui.press(gui.app, 'Start Dyno at login', c.role) : false; },
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
      if (!(await gui.press(gui.app, '^Run benchmarks$'))) return { ok: false, detail: 'no Run benchmarks button' };
      const started = await gui.waitFor(async () => (await gui.texts()).some((t) => t.role === 'AXButton' && /Cancel$/.test(t.text)), 10_000);
      const text = (await gui.texts()).map((t) => t.text).filter((x) => /Run benchmarks|Sustained|Cancel|Running|Pass \d/i.test(x)).slice(0, 10).join(' | ');
      if (!started.ok) return { ok: false, detail: `the benchmark did not start within 10 s of Run benchmarks: ${text}` };
      await gui.waitFor(() => gui.press(gui.app, '^Processes$', 'AXLink'), 10_000);
      return { ok: true, detail: `benchmark running: ${text}` };
    },
    async stopLongJob() { await gui.press(gui.app, '^Benchmarks$', 'AXLink'); await gui.press(gui.app, '^Cancel$', 'AXButton'); },
    async jobsText() { return (await gui.texts()).map((t) => t.text).filter((x) => /Start Dyno at login|Running benchmarks|Cancell?ed|Working|Cancel$|jobs could not be read/i.test(x)).join(' | '); },
    async pressCancel() { return gui.press(gui.app, '^Cancel', 'AXButton'); },
  };
}

/** Linux: the AT-SPI helper (`atspi.py`) through `atspi(args)` -> { code, stdout }. */
/**
 * The lines inside the open Jobs panel of the Linux AT-SPI tree (`landmark  Jobs` and everything nested under it), or null when the panel is closed. The status bar
 * button says "1 running: Running benchmarks" too, so a read of the whole window would "see" a job in a closed list.
 */
export function jobsPanelText(tree) {
  const lines = String(tree ?? '').split('\n');
  const at = lines.findIndex((l) => /^\s*landmark\s+Jobs\s*$/.test(l));
  if (at < 0) return null;
  const indent = (l) => l.length - l.trimStart().length;
  const base = indent(lines[at]);
  const inside = [];
  for (const l of lines.slice(at + 1)) {
    // a tree line is "<indent><role>  <name>" (two spaces); a continuation of a multi-line name has none
    if (l.trim() && indent(l) <= base && /\S {2}/.test(l)) break;
    if (l.trim()) inside.push(l.trim());
  }
  return inside.join('\n');
}

export function linuxLoginJobsDriver(atspi, parseSwitchState, { settle = (ms) => new Promise((r) => setTimeout(r, ms)) } = {}) {
  const SW = '"Start Dyno at login" "check box|toggle button|switch"';
  return {
    // Read in place; when the page is not there (Settings was closed to look at the Jobs list), Settings is opened the way a person does and read again.
    async readSwitch() {
      const read = () => { const r = atspi(`state dyno ${SW}`); return r.code === 0 ? parseSwitchState(r.stdout) : 'unknown'; };
      const first = read();
      if (first !== 'unknown') return first;
      atspi('press dyno "^File$" "menu item"'); await settle(1000); atspi('press dyno "^Settings…$" "menu item"'); await settle(4000); atspi('press dyno "^Dyno$" "push button"'); await settle(1500);
      for (let i = 0; i < 3; i++) { const v = read(); if (v !== 'unknown') return v; await settle(2000); }
      return 'unknown';
    },
    async pressSwitch() {
      if (atspi(`press dyno ${SW}`).code === 0) return true;
      atspi('press dyno "^File$" "menu item"'); await new Promise((r) => setTimeout(r, 1000)); atspi('press dyno "^Settings…$" "menu item"'); await new Promise((r) => setTimeout(r, 4000)); atspi('press dyno "^Dyno$" "push button"'); await new Promise((r) => setTimeout(r, 1500));
      return atspi(`press dyno ${SW}`).code === 0;
    },
    async save() { return atspi('press dyno "^Save changes$" "push button"').code === 0; },
    async apply() {
      if (atspi('press dyno "^Save changes$" "push button"').code !== 0) return { ok: false, detail: 'no "Save changes" button' };
      // Saved when "Applying…" and "Save changes" are gone (no draft is left). Read in place: closing and reopening Settings over AT-SPI lost the switch on the 0ad rounds
      // (rows 6 and 7 read "unknown" right after a reopen while the switch was on the page and Dyno.desktop was there).
      let pending = true;
      for (let i = 0; i < 15 && pending; i++) { await new Promise((r) => setTimeout(r, 2000)); pending = /Applying…|push button\s+Save changes/.test(atspi('tree dyno 60').stdout); }
      let v = 'unknown';
      for (let i = 0; i < 5 && v === 'unknown'; i++) { const r = atspi(`state dyno ${SW}`); v = r.code === 0 ? parseSwitchState(r.stdout) : 'unknown'; if (v === 'unknown') await new Promise((res) => setTimeout(res, 2000)); }
      return { ok: !pending, saved: v, detail: pending ? '"Save changes" or "Applying…" still shown after 30 s' : v === 'unknown' ? 'saved; the switch could not be read afterwards' : 'saved (no draft left)' };
    },
    // The status button toggles the panel: it is pressed only when the panel is not open (a second call used to close the list it had just opened).
    async openJobs() {
      atspi('press dyno "^Close$" "push button"');
      const isOpen = () => jobsPanelText(atspi('tree dyno 80').stdout) !== null;
      if (isOpen()) return true;
      for (let i = 0; i < 3; i++) { atspi(`press dyno ${JSON.stringify(STATUS)} "push button|toggle button"`); await new Promise((r) => setTimeout(r, 1500)); if (isOpen()) return true; }
      return false;
    },
    async startLongJob() {
      atspi('press dyno "^Close$" "push button"');
      if (atspi('press dyno "^Benchmarks$" "link"').code !== 0) return false;
      let ok = false;
      for (let i = 0; i < 30 && !ok; i++) { ok = atspi('press dyno "^Sustained processor" "radio button|toggle button|push button"').code === 0; if (!ok) { await new Promise((r) => setTimeout(r, 2000)); if (i % 4 === 3) atspi('press dyno "^Benchmarks$" "link"'); } } // the page appears a little after the link is pressed under Rosetta: wait on it, never spin
      const lines = () => atspi('tree dyno 80').stdout.split('\n').map((l) => l.trim()).filter((l) => /Run benchmarks|Sustained|Cancel|Running|Pass \d|Tick Yes|not|blocked/i.test(l)).slice(0, 12).join(' | ');
      if (!ok) return { ok: false, detail: `no "Sustained processor" choice on the Benchmarks page: ${lines()}` };
      if (atspi('press dyno "Run benchmarks$" "push button"').code !== 0) return { ok: false, detail: `no Run benchmarks button: ${lines()}` };
      let started = false;
      for (let i = 0; i < 10 && !started; i++) { await new Promise((r) => setTimeout(r, 1000)); started = /push button\s+.*Cancel/.test(atspi('tree dyno 80').stdout); }
      const text = lines();
      if (!started) return { ok: false, detail: `the benchmark did not start within 10 s of Run benchmarks: ${text}` };
      atspi('press dyno "^Processes$" "link"');
      return { ok: true, detail: `benchmark running: ${text}` };
    },
    async stopLongJob() { atspi('press dyno "^Benchmarks$" "link"'); atspi('press dyno "^Cancel$" "push button"'); },
    async jobsText() { return (jobsPanelText(atspi('tree dyno 80').stdout) ?? '').split('\n').filter((l) => /Start Dyno at login|Running benchmarks|Cancell?ed|Working|Cancel|jobs could not be read/i.test(l)).map((l) => l.trim()).join(' | '); },
    async pressCancel() { return atspi('press dyno "^Cancel" "push button"').code === 0; },
  };
}

export { loginJobsRow };
