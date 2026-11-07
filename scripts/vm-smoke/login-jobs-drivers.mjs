// The per-OS drivers of row 5a (login-jobs-row.mjs). Each reads and presses "Start Dyno at login" in Settings and reads the Jobs list through the OS's own
// accessibility API. The shell's Jobs list is a `ul.pd-jobs` (aria-label from the shell) whose rows carry the job name, its status and a Cancel button
// (ui/projects/paddock-ui/shell/jobs/src/jobs-list.ts). Names are matched loosely (/Jobs/, /^Cancel/) because they are the shell's, not Dyno's.
import { loginJobsRow } from './login-jobs-row.mjs';

const SWITCH = /Start Dyno at login/i;

/** Windows: Playwright over the WebView2 DevTools port. `openSettings(browser)` returns the Settings page. */
export function windowsLoginJobsDriver(browser, { openSettings, mainPage }) {
  const settings = async () => openSettings(browser);
  const box = async () => { const s = await settings(); return s ? s.getByRole('switch', { name: SWITCH }).or(s.getByRole('checkbox', { name: SWITCH })).first() : null; };
  return {
    async readSwitch() { const b = await box(); if (!b || !(await b.count().catch(() => 0))) return 'unknown'; const v = await b.getAttribute('aria-checked').catch(() => null); return v === 'true' ? 'on' : v === 'false' ? 'off' : 'unknown'; },
    async pressSwitch() { const b = await box(); if (!b) return false; return b.click().then(() => true, () => false); },
    async openJobs() {
      const p = mainPage(browser); if (!p) return false;
      if (await p.locator('ul.pd-jobs').count()) return true;
      const item = p.getByRole('button', { name: /jobs/i }).first();
      if (await item.count()) await item.click().catch(() => {});
      return (await p.locator('ul.pd-jobs').count()) > 0;
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
    async openJobs() { await gui.ensureMainWindow(); return (await gui.has(gui.app, '^Jobs', 'AXButton')) ? gui.press(gui.app, '^Jobs', 'AXButton') : (await gui.texts()).some((t) => /Jobs/i.test(t.text)); },
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
    async openJobs() { return atspi('press dyno "Jobs" "push button"').code === 0 || /Jobs/.test(atspi('tree dyno 60').stdout); },
    async jobsText() { return atspi('tree dyno 80').stdout.split('\n').filter((l) => /Start Dyno at login|Cancell?ed|Working|Cancel/i.test(l)).map((l) => l.trim()).join(' | '); },
    async pressCancel() { return atspi('press dyno "^Cancel" "push button"').code === 0; },
  };
}

export { loginJobsRow };
