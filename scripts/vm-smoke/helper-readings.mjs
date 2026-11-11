// WP-VMH: one step per helper reading (docs/plans/2026-11-04-helper-wiring.md section 5), as data. Each reading names the page that shows it and what
// the page must say once the reading is wired through the helper: the `via helper` source label, the counts or columns, and the honest reason where
// the VM cannot supply the data (no DMI, no CPU power, no backup versions). A step opens the page through a driver, reads its text and judges the
// expectations; it fails with the missing text, so a package that has not landed fails with a readable cause. Adding a reading adds a row here.
//
// A driver is { openPage(page): Promise<boolean>, text(): Promise<string>, run?(cmd): Promise<string> }:
// Windows over the WebView2 DevTools port (win-ui.mjs), macOS through the Accessibility tree (mac-gui.mjs), Linux when its guest exists.

/**
 * Wording of a page that has NOT taken the reading (the helper is dead, a tool could not run). A reading page that says any of it never passes,
 * whatever else is on it, unless the row names it in `allow` because the honest VM reason uses it. (The audit's false passes: Lifespan with a dead
 * helper, Backup "could not check".)
 */
export const FAILURE_WORDING = [/did not answer/i, /could not check/i, /\bRepair it\b/i, /cannot be estimated/i, /Could not read what keeps/i, /needs the helper/i, /\bNeeds repair\b/i, /\bNot running\b/i];

/** A real wear or lifespan value of the Lifespan page (and of the Windows disk-health step): a forecast, a progress of history, or "no wear". */
export const LIFESPAN_VALUE = /Have \d+ of \d+ days|\b\d+(?:\.\d+)? (?:years?|months?) of writes left|Replace in about \d+|No wear (?:was )?detected|Soonest \d|Expected \d/i;

/** page: a route name of Dyno's router (#/<page>), `tab`: a tab or radio label to press first. `all` must all match, `none` must not match. */
export const READINGS = [
  { id: 'processes-all-accounts', title: 'Processes of every account', os: ['windows', 'macos', 'linux'], page: 'processes', showAll: true, fullText: 4000, shot: 'processes', minWidth: 1100,
    expect: { windows: [/Account/, /SYSTEM|NT AUTHORITY/, /Read by the administrator helper/i], macos: [/root/, /_[a-z]+/, /Read by the administrator helper/i], linux: [/root/, /Read by the administrator helper/i] }, none: [/needs the helper/i] },
  { id: 'traffic-by-program', title: 'Per-program network traffic (Group by: Program)', os: ['windows'], page: 'network', groupBy: 'Program',
    expect: { windows: [/Read by the administrator helper/i] }, none: [/needs the helper/i], prepare: { windows: 'traffic' },
    // A program row with a transfer size (not the column header glyph): the control generates traffic first.
    value: { windows: [/\b[\w.-]+\.exe\b.{0,120}?\d[\d.,]*\s?(?:[KMG]i?B|B)\b/i] } },
  { id: 'wake-requests', title: 'What keeps the computer awake (wake requests)', os: ['windows'], page: 'extras', tab: 'Sleep',
    expect: { windows: [/Read by the administrator helper/i] }, none: [/needs the helper/i], prepare: { windows: 'awake-on' },
    // The control holds the computer awake with `dyno awake on`, so the blockers table must list a Dyno request (not only the chip).
    value: { windows: [/Keeping this computer awake\s+[1-9]\d* programs?/i, /Program\s+It prevents[\s\S]*\bDyno\b/i] } },
  { id: 'backup-versions', title: 'Windows Backup versions', os: ['windows'], page: 'backup', fullText: 4000,
    expect: { windows: [/Read by the administrator helper/i] }, none: [/needs the helper/i],
    // Per row (fix/dyno-vm-findings-1, efe366863): a File History log that never ran reads "Not set up" with a next step, which is right in a VM; "Could not check" is for a real tool failure. A build before that commit still says "Could not check 2 backup tools" and keeps failing.
    value: { windows: [/no backup of this PC|Windows Backup has \d+ versions?|Not set up/i] }, allowNotSetUp: true },
  { id: 'login-items', title: 'macOS login items (BTM) beside launch agents', os: ['macos'], page: 'health', fullText: 4000,
    // Measured 2026-11-05 on run-7575: login items are listed on Health > Start-up items (Extras > Boot has none). With the helper On the page said
    // "Login items ... need administrator rights to read" because the agent only ever builds NeedsHelperBtm (startup.rs:178); that is the product gap this row catches.
    expect: { macos: [/Start-up items/i] }, none: [/needs the helper/i, /Login items[^.]*need administrator rights/i] },
  { id: 'firewall', title: 'Firewall rules (pf on macOS, nft on Linux)', os: ['macos', 'linux'], page: 'firewall',
    expect: { macos: [/Read by the administrator helper/i], linux: [/Read by the administrator helper/i] }, none: [/needs the helper/i],
    // A source named in full and a non-zero rule count ("0 rules" read nothing). The control loads one known rule first, on Linux an nft rule (tcp dport 59999) and
    // on macOS a pf rule on the same port: the image has an empty pf ruleset, so without the control the page rightly says "0 rules" (2026-11-05: with the rule it says "2 rules", "Ports = 59999").
    value: { macos: [/\bpacket filter \(pf\)[\s\S]*\b[1-9][\d,]* rules?\b/i, /59999/], linux: [/\bLinux nftables[\s\S]*\b[1-9][\d,]* rules?\b/i, /59999/] }, prepare: { linux: 'nft-rule', macos: 'pf-rule' } },
  { id: 'energy-and-thermal', title: 'macOS CPU energy: the VM reason on the Energy tab', os: ['macos'], page: 'power', tab: 'Energy', fullText: 4000,
    // The reason is the helper's own text (helper_counters.rs "does not report CPU power (powermetrics listed none, as in a virtual machine) to software"); the Energy tab carries no "Read by" label.
    expect: { macos: [/does not report CPU power \(powermetrics listed none, as in a virtual machine\)/i] }, none: [/needs the helper/i] },
  { id: 'sockets-and-holders', title: 'Linux socket owners and file holders', os: ['linux'], page: 'network',
    expect: { linux: [/Read by the administrator helper/i] }, none: [/needs the helper/i, /\bunknown program\b/i] },
  { id: 'memory-modules', title: 'Linux memory modules (no DMI in a VM: the reason)', os: ['linux'], page: 'inventory',
    expect: { linux: [/Memory modules|installed/i] }, none: [/needs the helper/i],
    // Modules with a size, or the installed total with the honest reason (no DMI in a VM). The heading alone is not a reading.
    value: { linux: [/\b\d[\d.,]*\s?(?:GB|GiB|MB|MiB)\b/i] }, allow: [/could not check/i, /Dyno could not read the individual modules/i] },
  { id: 'firewall-windows', title: 'Windows Defender Firewall is read without the helper (COM)', os: ['windows'], page: 'firewall',
    expect: { windows: [/Windows Defender Firewall/, /Domain/, /Private/, /Public/] }, none: [] },
  { id: 'wol-state', title: 'Wake devices readiness (WolState)', os: ['linux'], page: 'power', tab: 'Wake devices',
    expect: { linux: [/This computer/] }, none: [/needs the helper/i],
    // At least one finding with a verdict word; only "Can't tell" is the VM saying it cannot read the settings: not measured, not a pass.
    value: { linux: [/\b(?:Fine|Needs a change)\b/] }, notMeasured: { linux: { when: (t) => /Can't tell/.test(t) && !/\b(?:Fine|Needs a change)\b/.test(t), why: 'the VM exposes no wake settings (every finding says Can\'t tell)' } } },
  { id: 'thermal-pressure', title: 'macOS temperatures: the thermal pressure reason on a Temperature widget', os: ['macos'], page: 'dashboard', fullText: 4000,
    expect: { macos: [/macOS reports a thermal pressure level, not temperatures/i] }, none: [/needs the helper/i],
    // The sentence is the widget's own reason (widget-view.ts on wp-int: "Not available. <reason>" under a Big number or Graph whose metric is Temperature). The default dashboard has no
    // such widget, so the control adds one and sets its metric (macDriver 'temp-widget'); measured 2026-11-05 on run-7575 it then reads "macOS reports a thermal pressure level, not temperatures, to software."
    prepare: { macos: 'temp-widget' } },
  { id: 'lifespan-smart', title: 'Disk health reading (Lifespan)', os: ['windows', 'linux'], page: 'lifespan',
    expect: { windows: [/SSD/], linux: [/SSD/] }, none: [/not installed/i, /needs the helper/i], value: { windows: [LIFESPAN_VALUE], linux: [LIFESPAN_VALUE] },
    // The VM base has no smartmontools: the reading cannot be taken there, which is neither a pass nor a fail of the product.
    notMeasured: { linux: { when: /smartctl[^.]*is not installed/i, why: 'smartmontools missing in base' } } },
];

/** A row's `notMeasured` entry fires on a page text: `when` is a regex or a function of the text. */
const fires = (nm, text) => (typeof nm.when === 'function' ? nm.when(text) : nm.when.test(text));

/**
 * One reading on one OS as a step { title, status, summary, checks, evidence }. A page passes only with (1) its chip or label, (2) a real value
 * of the row (`value`: a count, a size, a wear figure, never a heading or an intro sentence), (3) none of the failure wording the helper being
 * dead or a tool failing puts on the page. The row's control (`prepare`) runs first when the driver has one; a control that fails makes the step
 * `check-broken`, never a product fail.
 */
export async function readingStep(reading, os, driver) {
  const title = `Helper reading (${os}): ${reading.title}`;
  const checks = [];
  const evidence = [];
  const control = reading.prepare?.[os];
  if (control && driver.prepare) {
    const c = await driver.prepare(control).catch((e) => ({ ok: false, detail: String(e.message ?? e) }));
    checks.push({ ok: c.ok, kind: 'check', detail: c.ok ? `control "${control}" ran: ${c.detail ?? 'done'}` : `the control "${control}" could not run: ${c.detail ?? ''}` });
    if (!c.ok) return { title, status: 'check-broken', cls: 'check', summary: checks[0].detail, checks, evidence };
  } else if (control) checks.push({ ok: true, info: true, detail: `no driver for the control "${control}": the row is judged without it` });
  try {
    const opened = await driver.openPage(reading.page, reading.tab, reading);
    checks.push({ ok: opened, detail: opened ? `opened ${reading.page}${reading.tab ? ` > ${reading.tab}` : ''}` : `could not open ${reading.page}${reading.tab ? ` > ${reading.tab}` : ''}` });
    if (!opened) return { title, status: 'fail', summary: checks.at(-1).detail, checks, evidence: driver.why ? [driver.why] : evidence };
    const text = (await driver.text(reading)).replace(/\s+/g, ' ');
    evidence.push(`page text: ${text.slice(0, reading.fullText ?? 3000)}`);
    if (reading.shot && driver.shot) { const f = await driver.shot(`page-${reading.shot}`); if (f) evidence.push(`screenshot: ${f}`); }
    const nm = reading.notMeasured?.[os];
    if (nm && fires(nm, text)) {
      const detail = `not measured: ${nm.why}${nm.when instanceof RegExp ? ` (the page says ${nm.when})` : ''}`;
      return { title, status: 'not-measured', summary: `not measured: ${nm.why}`, checks: [...checks, { ok: true, info: true, detail }], evidence };
    }
    for (const re of reading.expect[os] ?? []) checks.push({ ok: re.test(text), detail: re.test(text) ? `the page says ${re}` : `the page does not say ${re}` });
    for (const re of reading.value?.[os] ?? []) checks.push({ ok: re.test(text), detail: re.test(text) ? `the page shows a value (${re})` : `the page shows no value: nothing matches ${re}` });
    for (const re of reading.none ?? []) checks.push({ ok: !re.test(text), detail: re.test(text) ? `the page still says ${re} (the reading is not wired through the helper)` : `the page does not say ${re}` });
    // `allowNotSetUp`: sentences about File History that say "Not set up" are not a failure of the reading; everything else is still judged.
    const judged = reading.allowNotSetUp ? text.split(/(?<=[.!?])\s+/).filter((s) => !(/File History/i.test(s) && /Not set up/i.test(s))).join(' ') : text;
    for (const re of FAILURE_WORDING) {
      if ((reading.allow ?? []).some((a) => a.source === re.source) || (reading.none ?? []).some((n) => n.source === re.source)) continue;
      checks.push({ ok: !re.test(judged), detail: re.test(judged) ? `the page says ${re}: the helper did not take the reading` : `the page does not say ${re}` });
    }
  } finally {
    if (control && driver.cleanup) await Promise.resolve(driver.cleanup(control)).catch(() => {});
  }
  const bad = checks.filter((c) => !c.ok);
  return { title, status: bad.length ? 'fail' : 'pass', summary: bad.length ? bad.map((c) => c.detail).join('; ').slice(0, 240) : 'the page shows the reading through the helper', checks, evidence };
}

/** The extension hooks for one OS: one function per reading that applies to it. */
export function readingSteps(os, makeDriver) {
  return READINGS.filter((r) => r.os.includes(os)).map((r) => async (ctx) => readingStep(r, os, await makeDriver(ctx)));
}

/** Windows: the driver over a Playwright page of the main window (hash routes; tabs are pressed by their label). */
export function windowsDriver(main, { shotDir = process.env.PADDOCK_WIN_SHOTS ?? null, ssh = null } = {}) {
  return {
    /** The controls: traffic from the guest, a Dyno keep-awake request. They need the guest shell (`ssh`); without it the row is judged without a control. */
    async prepare(key) {
      if (!ssh) return { ok: false, detail: 'no guest shell for the control' };
      if (key === 'traffic') { ssh('1..6 | ForEach-Object { curl.exe -s -m 8 -o NUL https://dev.decryptology.net/ }'); return { ok: true, detail: 'six requests to the update host' }; }
      if (key === 'awake-on') { const r = ssh('& "$env:ProgramFiles\\Dyno\\dyno.exe" awake on --for 1h 2>&1 | Out-String; "EXIT=$LASTEXITCODE"'); return /EXIT=0\s*$/.test(r) ? { ok: true, detail: 'dyno awake on --for 1h' } : { ok: false, detail: `dyno awake on: ${r.slice(-120)}` }; }
      return { ok: false, detail: `unknown control ${key}` };
    },
    async cleanup(key) { if (ssh && key === 'awake-on') ssh('& "$env:ProgramFiles\\Dyno\\dyno.exe" awake off 2>&1 | Out-Null'); },
    async openPage(page, tab, reading = {}) {
      if (reading.minWidth) {
        // The table needs width for its Account column: ask the window for at least that many pixels (DevTools Browser.setWindowBounds).
        try { const s = await main.context().newCDPSession(main); const { windowId } = await s.send('Browser.getWindowForTarget'); await s.send('Browser.setWindowBounds', { windowId, bounds: { windowState: 'normal', width: reading.minWidth + 100, height: 820 } }); } catch { /* a window that cannot be resized keeps its size */ }
        await new Promise((r) => setTimeout(r, 800));
      }
      await main.evaluate((p) => { location.hash = `#/${p}`; }, page);
      await new Promise((r) => setTimeout(r, 2500));
      if (tab) { const t = main.getByRole('tab', { name: new RegExp(`^${tab}`) }).or(main.getByRole('radio', { name: new RegExp(`^${tab}`) })).or(main.getByRole('button', { name: new RegExp(`^${tab}`) })); if (!(await t.count())) return false; await t.first().click(); await new Promise((r) => setTimeout(r, 1500)); }
      if (reading.showAll) { const all = main.getByRole('button', { name: /^All$/ }).or(main.getByRole('radio', { name: /^All$/ })).or(main.getByRole('tab', { name: /^All$/ })); if (await all.count()) { await all.first().click(); await new Promise((r) => setTimeout(r, 2500)); } }
      if (reading.groupBy) {
        // "Group by" is a segmented control (network-page.ts), not a combobox: the options are radios or buttons named None, Program and, on the Connections tab, a remote-host one.
        const opt = main.getByRole('radio', { name: new RegExp(`^${reading.groupBy}`) }).or(main.getByRole('button', { name: new RegExp(`^${reading.groupBy}`) })).or(main.getByRole('tab', { name: new RegExp(`^${reading.groupBy}`) }));
        if (!(await opt.count())) { this.why = `no "${reading.groupBy}" option in the Group by control; controls on the page: ` + (await main.evaluate(() => [...document.querySelectorAll('[role=radio], [role=tab], [role=radiogroup], button')].map((e) => `${e.getAttribute('role') ?? e.tagName.toLowerCase()}:${(e.getAttribute('aria-label') || e.textContent || '').trim().replace(/\s+/g, ' ').slice(0, 40)}`).filter((x) => !x.endsWith(':')).slice(0, 40).join(' | '))); return false; }
        await opt.first().click(); await new Promise((r) => setTimeout(r, 2500));
      }
      return true;
    },
    text: () => main.evaluate(() => document.body.innerText),
    async shot(name) {
      if (!shotDir) return null;
      try { const { mkdirSync, writeFileSync } = await import('node:fs'); mkdirSync(shotDir, { recursive: true }); const f = `dom-${name}.png`; writeFileSync(`${shotDir}/${f}`, await main.screenshot()); return f; } catch { return null; }
    },
  };
}

/** macOS: the driver over a MacGui (sidebar links by label; tabs are radio buttons). */
export function macDriver(gui) {
  let loadedPf = false;
  const label = { dashboard: 'Dashboard', processes: 'Processes', network: 'Network', extras: 'Extras', backup: 'Backup health', health: 'Health', firewall: 'Firewall', power: 'Power', inventory: 'Inventory', lifespan: 'Lifespan' };
  return {
    /** Controls: `pf-rule` loads one known rule into the guest's (empty) pf ruleset, without enabling pf; `cleanup` restores /etc/pf.conf. */
    async prepare(key) {
      if (key === 'temp-widget') return addTemperatureWidget(gui);
      if (key !== 'pf-rule') return { ok: false, detail: `unknown control ${key}` };
      const r = await gui.g.exec(`echo 'block drop quick proto tcp from any to any port 59999' | sudo /sbin/pfctl -f - 2>&1; sudo /sbin/pfctl -sr 2>&1 | grep -c 59999`);
      loadedPf = /^1$/m.test(r.stdout.trim().split('\n').at(-1) ?? '');
      return loadedPf ? { ok: true, detail: 'pf rule on tcp port 59999 loaded' } : { ok: false, detail: `pfctl did not list the rule: ${r.stdout.trim().slice(-120)}` };
    },
    async cleanup(key) { if (key === 'temp-widget') return removeTemperatureWidget(gui); if (key === 'pf-rule' && loadedPf) { await gui.g.exec('sudo /sbin/pfctl -f /etc/pf.conf >/dev/null 2>&1; true'); loadedPf = false; } },
    async openPage(page, tab, reading = {}) {
      await gui.closeSettings(); // the Settings window stays open after the helper step and would be read as the page
      if (reading.minWidth && (await gui.has(gui.app, '^Collapse sidebar'))) await gui.press(gui.app, '^Collapse sidebar'); // room for the Account column
      const ok = (await gui.waitFor(() => gui.press(gui.app, `^${label[page]}$`, 'AXLink'), 15_000)).ok;
      if (ok && tab) return (await gui.waitFor(async () => (await gui.press(gui.app, `^(${tab})( |$)`, 'AXRadioButton')) || (await gui.press(gui.app, `^(${tab})( |$)`, 'AXButton')), 10_000)).ok;
      return ok;
    },
    /**
     * The page's text. A page reads its helper data a few seconds after it opens (the firewall page said "0 rules" with no "Read by" label right after the click, and
     * showed both 12 s later), so when the row names what the page must say, it is polled until all of it is there, bounded at 30 s (a condition, not a delay). What is
     * still missing after that is judged as it stands.
     */
    async text(reading = {}) {
      const read = async () => (await gui.texts()).map((t) => t.text).join(' ');
      const want = [...(reading.expect?.macos ?? []), ...(reading.value?.macos ?? [])];
      if (!want.length) return read();
      const got = await gui.waitFor(async () => { const t = await read(); return want.every((re) => re.test(t)) ? t : null; }, 30_000);
      return got.ok ? got.value : read();
    },
  };
}

/**
 * Control for the thermal row: a Big number widget with the metric Temperature (Edit layout > Add widget > Add Big number > its Options > Metric). The new widget is the second
 * "Actions for CPU" (the default layout has one CPU card). The option is chosen with AXPress, which was measured to change the combobox like a real click (2026-11-05: AXPress on
 * Swap and a pointer click on Disk activity both changed it); what failed earlier was pressing the first "Actions for CPU", the card, not the new widget.
 */
export async function addTemperatureWidget(gui) {
  const must = async (what, fn) => { if (!(await gui.waitFor(fn, 15_000)).ok) throw new Error(what); };
  try {
    await gui.closeSettings();
    await must('no Dashboard link', () => gui.press(gui.app, '^Dashboard$', 'AXLink'));
    if (await gui.has(gui.app, '^Collapse sidebar')) await gui.press(gui.app, '^Collapse sidebar');
    await must('no "Edit layout" button', () => gui.press(gui.app, '^Edit layout$'));
    await must('no "Add widget" button', () => gui.press(gui.app, '^Add widget'));
    await must('no "Add Big number" button', () => gui.press(gui.app, '^Add Big number'));
    await must('the new widget has no second "Actions for CPU" menu', () => gui.press(gui.app, '^Actions for CPU$', 'AXPopUpButton', 1));
    await must('no "Options for CPU…" menu item', () => gui.press(gui.app, '^Options for CPU', 'AXMenuItem'));
    await must('no Metric combobox in the widget options', () => gui.press(gui.app, '^Metric of', 'AXComboBox'));
    await must('no "Temperature" option in the Metric list', () => gui.press(gui.app, '^Temperature$', 'AXStaticText'));
    await must('the widget options did not take the metric Temperature', async () => { const t = await gui.texts(); const i = t.findIndex((x) => x.text === 'Widget options'); return i >= 0 && t[i + 1]?.text === 'Temperature'; });
    await gui.press(gui.app, '^Done$', 'AXButton'); // the options panel
    await gui.waitFor(() => gui.press(gui.app, '^Done$', 'AXButton'), 10_000); // the layout editor
    return { ok: true, detail: 'a Big number widget with the metric Temperature was added to the dashboard' };
  } catch (e) {
    await gui.press(gui.app, '^Done$', 'AXButton').catch(() => {});
    return { ok: false, detail: String(e.message ?? e) };
  }
}

/** Takes the widget the control added off the dashboard again (its menu is named after its metric now). */
export async function removeTemperatureWidget(gui) {
  try {
    await gui.press(gui.app, '^Edit layout$');
    await gui.waitFor(() => gui.press(gui.app, '^Actions for Temperature$', 'AXPopUpButton'), 10_000);
    await gui.waitFor(() => gui.press(gui.app, '^Remove Temperature$', 'AXMenuItem'), 10_000);
  } finally { await gui.press(gui.app, '^Done$', 'AXButton').catch(() => {}); }
}
