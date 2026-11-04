// WP-VMH: one step per helper reading (docs/plans/2026-11-04-helper-wiring.md section 5), as data. Each reading names the page that shows it and what
// the page must say once the reading is wired through the helper: the `via helper` source label, the counts or columns, and the honest reason where
// the VM cannot supply the data (no DMI, no CPU power, no backup versions). A step opens the page through a driver, reads its text and judges the
// expectations; it fails with the missing text, so a package that has not landed fails with a readable cause. Adding a reading adds a row here.
//
// A driver is { openPage(page): Promise<boolean>, text(): Promise<string>, run?(cmd): Promise<string> }:
// Windows over the WebView2 DevTools port (win-ui.mjs), macOS through the Accessibility tree (mac-gui.mjs), Linux when its guest exists.

/** page: a route name of Dyno's router (#/<page>), `tab`: a tab or radio label to press first. `all` must all match, `none` must not match. */
export const READINGS = [
  { id: 'processes-all-accounts', title: 'Processes of every account', os: ['windows', 'macos', 'linux'], page: 'processes',
    expect: { windows: [/Account/, /SYSTEM/, /via helper/i], macos: [/root/, /_[a-z]+/, /via helper/i], linux: [/root/, /via helper/i] }, none: [/needs the helper/i] },
  { id: 'traffic-by-program', title: 'Per-program network traffic', os: ['windows'], page: 'network', tab: 'By program',
    expect: { windows: [/By program/, /via helper/i] }, none: [/needs the helper/i] },
  { id: 'wake-requests', title: 'What keeps the computer awake (wake requests)', os: ['windows'], page: 'extras', tab: 'Sleep',
    expect: { windows: [/via helper/i] }, none: [/needs the helper/i] },
  { id: 'backup-versions', title: 'Windows Backup versions', os: ['windows'], page: 'backup',
    expect: { windows: [/no (backup )?versions|has no versions|no Backup and Restore/i] }, none: [/needs the helper/i] },
  { id: 'login-items', title: 'macOS login items (BTM) beside launch agents', os: ['macos'], page: 'extras', tab: 'Startup',
    expect: { macos: [/Login items|Background items/i, /via helper/i] }, none: [/needs the helper/i] },
  { id: 'firewall', title: 'Firewall rules (pf on macOS, nft on Linux)', os: ['macos', 'linux'], page: 'firewall',
    expect: { macos: [/pf/i, /rules?/i, /via helper/i], linux: [/nft|nftables/i, /rules?/i, /via helper/i] }, none: [/needs the helper/i] },
  { id: 'energy-and-thermal', title: 'macOS CPU energy and thermal pressure (honest reasons in a VM)', os: ['macos'], page: 'power', tab: 'Energy',
    expect: { macos: [/CPU power|energy/i] }, none: [/needs the helper/i] },
  { id: 'sockets-and-holders', title: 'Linux socket owners and file holders', os: ['linux'], page: 'network',
    expect: { linux: [/via helper/i] }, none: [/needs the helper/i, /\bunknown program\b/i] },
  { id: 'memory-modules', title: 'Linux memory modules (no DMI in a VM: the reason)', os: ['linux'], page: 'inventory',
    expect: { linux: [/DMI|memory modules/i] }, none: [/needs the helper/i] },
  { id: 'lifespan-smart', title: 'Disk health reading (Lifespan)', os: ['windows', 'linux'], page: 'lifespan',
    expect: { windows: [/SSD/], linux: [/SSD/] }, none: [/not installed/i, /needs the helper/i] },
];

/** One reading on one OS as a step { title, status, summary, checks, evidence }. */
export async function readingStep(reading, os, driver) {
  const title = `Helper reading (${os}): ${reading.title}`;
  const checks = [];
  const evidence = [];
  const opened = await driver.openPage(reading.page, reading.tab);
  checks.push({ ok: opened, detail: opened ? `opened ${reading.page}${reading.tab ? ` > ${reading.tab}` : ''}` : `could not open ${reading.page}${reading.tab ? ` > ${reading.tab}` : ''}` });
  if (!opened) return { title, status: 'fail', summary: checks[0].detail, checks, evidence };
  const text = (await driver.text()).replace(/\s+/g, ' ');
  evidence.push(`page text: ${text.slice(0, 400)}`);
  for (const re of reading.expect[os] ?? []) checks.push({ ok: re.test(text), detail: re.test(text) ? `the page says ${re}` : `the page does not say ${re}` });
  for (const re of reading.none ?? []) checks.push({ ok: !re.test(text), detail: re.test(text) ? `the page still says ${re} (the reading is not wired through the helper)` : `the page does not say ${re}` });
  const bad = checks.filter((c) => !c.ok);
  return { title, status: bad.length ? 'fail' : 'pass', summary: bad.length ? bad.map((c) => c.detail).join('; ').slice(0, 240) : 'the page shows the reading through the helper', checks, evidence };
}

/** The extension hooks for one OS: one function per reading that applies to it. */
export function readingSteps(os, makeDriver) {
  return READINGS.filter((r) => r.os.includes(os)).map((r) => async (ctx) => readingStep(r, os, await makeDriver(ctx)));
}

/** Windows: the driver over a Playwright page of the main window (hash routes; tabs are pressed by their label). */
export function windowsDriver(main) {
  return {
    async openPage(page, tab) {
      await main.evaluate((p) => { location.hash = `#/${p}`; }, page);
      await new Promise((r) => setTimeout(r, 2500));
      if (tab) { const t = main.getByRole('tab', { name: new RegExp(`^${tab}`) }).or(main.getByRole('radio', { name: new RegExp(`^${tab}`) })).or(main.getByRole('button', { name: new RegExp(`^${tab}`) })); if (!(await t.count())) return false; await t.first().click(); await new Promise((r) => setTimeout(r, 1500)); }
      return true;
    },
    text: () => main.evaluate(() => document.body.innerText),
  };
}

/** macOS: the driver over a MacGui (sidebar links by label; tabs are radio buttons). */
export function macDriver(gui) {
  const label = { processes: 'Processes', network: 'Network', extras: 'Extras', backup: 'Backup health', firewall: 'Firewall', power: 'Power', inventory: 'Inventory', lifespan: 'Lifespan' };
  return {
    async openPage(page, tab) {
      const ok = (await gui.waitFor(() => gui.press(gui.app, `^${label[page]}$`, 'AXLink'), 15_000)).ok;
      if (ok && tab) return (await gui.waitFor(() => gui.press(gui.app, `^${tab}( |$)`, 'AXRadioButton'), 10_000)).ok;
      return ok;
    },
    async text() { return (await gui.texts()).map((t) => t.text).join(' '); },
  };
}
