import assert from 'node:assert/strict';
import { test } from 'node:test';
import { READINGS, linuxDriver, readingStep, readingSteps } from './helper-readings.mjs';

const driver = (text, opens = true) => ({ openPage: async () => opens, text: async () => text });
const by = (id) => READINGS.find((r) => r.id === id);

test('a wired reading passes, an unwired one fails with the missing text', async () => {
  const good = await readingStep(by('processes-all-accounts'), 'windows', driver('Processes Account Name SYSTEM svchost Read by the administrator helper'));
  assert.equal(good.status, 'pass');
  const unwired = await readingStep(by('processes-all-accounts'), 'windows', driver('Processes Account Name SYSTEM svchost Needs the helper'));
  assert.equal(unwired.status, 'fail');
  assert.match(unwired.summary, /administrator helper/);
  assert.match(unwired.summary, /needs the helper/i);
});
test('a page that cannot be opened fails', async () => {
  const s = await readingStep(by('firewall'), 'macos', driver('', false));
  assert.equal(s.status, 'fail');
  assert.match(s.summary, /could not open firewall/);
});
test('every reading names expectations for each OS it lists', () => {
  for (const r of READINGS) for (const os of r.os) assert.ok((r.expect[os] ?? []).length > 0, `${r.id} has no expectation for ${os}`);
});
test('hooks are produced per OS', () => {
  assert.equal(readingSteps('windows', async () => driver('')).length, READINGS.filter((r) => r.os.includes('windows')).length + 1); // + the removed Backup health row
  assert.ok(readingSteps('linux', async () => driver('')).length >= 4);
});

// Dead-state fixtures: pages of the audit (vmaudit/probe.mjs) that used to PASS while the helper read nothing. Each must not pass.
const run = async (id, os, text) => readingStep(by(id), os, driver(text));
const DEAD = [
  ['lifespan-smart', 'windows', 'Lifespan How long your SSD and battery are likely to last. SSD Disk life cannot be estimated The helper is installed but did not answer. Repair it to start it again.'],
  ['lifespan-smart', 'linux', 'Lifespan How long your SSD and battery are likely to last. SSD Disk life cannot be estimated'],
  ['lifespan-smart', 'windows', 'Lifespan How long your SSD and battery are likely to last.'],
  ['lifespan-smart', 'linux', 'Lifespan How long your SSD and battery are likely to last.'],
  ['memory-modules', 'linux', 'Inventory What is installed. Memory modules'],
  ['traffic-by-program', 'windows', 'Network ↓ 0 B/s Read by the administrator helper'],
  ['firewall', 'linux', 'Linux nftables Firewall: Off 0 rules Read by the administrator helper'],
  ['wake-requests', 'windows', 'Extras Read by the administrator helper'],
  ['wake-requests', 'windows', 'Extras Read by the administrator helper Keeping this computer awake Could not read what keeps this computer awake'],
  ['wol-state', 'linux', 'Power Wake computers by sending them a Wake-on-LAN packet.'],
  ['firewall', 'macos', 'Profile rule Read by the administrator helper'],
  ['energy-and-thermal', 'macos', 'Power Energy The helper listed no CPU power, as in a virtual machine. Dyno cannot check: Repair it'],
];
for (const [id, os, text] of DEAD) {
  test(`dead state must not pass: ${id} on ${os}: ${text.slice(0, 50)}`, async () => {
    const r = await run(id, os, text);
    assert.notEqual(r.status, 'pass', r.summary);
  });
}
test('real values pass: a wear forecast, a backup count, memory modules, a traffic row, a loaded nft rule, a Dyno blocker, a verdict', async () => {
  assert.equal((await run('lifespan-smart', 'windows', 'Lifespan SSD Samsung About 7.4 years of writes left (5.2 to 10 years)')).status, 'pass');
  assert.equal((await run('lifespan-smart', 'linux', 'Lifespan SSD vda Not enough history for a forecast yet Have 6 of 14 days')).status, 'pass');
  assert.equal((await run('memory-modules', 'linux', 'Inventory Memory modules Slot Size Part number DIMM0 8 GB')).status, 'pass');
  assert.equal((await run('memory-modules', 'linux', 'Inventory Memory modules 4 GB installed Dyno could not read the individual modules: no DMI data in this virtual machine')).status, 'pass');
  assert.equal((await run('traffic-by-program', 'windows', 'Network Program ↓ ↑ curl.exe 1.2 MB 40 KB Read by the administrator helper')).status, 'pass');
  assert.equal((await run('firewall', 'linux', 'Linux nftables 2 rules tcp dport 59999 accept Read by the administrator helper')).status, 'pass');
  assert.equal((await run('wake-requests', 'windows', 'Extras Read by the administrator helper Keeping this computer awake 1 program Program It prevents Reason Held for Dyno The system from sleeping 4 s')).status, 'pass');
  assert.equal((await run('wol-state', 'linux', 'Power This computer What Dyno can read Wake for network Fine')).status, 'pass');
});
test('a wake-state page of only "Can\'t tell" is not measured, never a pass', async () => {
  assert.equal((await run('wol-state', 'linux', "Power This computer Wake for network Can't tell")).status, 'not-measured');
});
test('a control that cannot run makes the step check-broken, not a product fail, and is cleaned up', async () => {
  const calls = [];
  const d = { ...driver('Linux nftables 2 rules 59999 Read by the administrator helper'), prepare: async () => ({ ok: false, detail: 'sudo nft failed' }), cleanup: async (k) => calls.push(k) };
  const r = await readingStep(by('firewall'), 'linux', d);
  assert.equal(r.status, 'check-broken');
  const d2 = { ...driver('Linux nftables 2 rules 59999 Read by the administrator helper'), prepare: async () => ({ ok: true }), cleanup: async (k) => calls.push(k) };
  assert.equal((await readingStep(by('firewall'), 'linux', d2)).status, 'pass');
  assert.deepEqual(calls, ['nft-rule']);
});

test('Backup is judged per row: "Not set up" for File History passes, Windows Backup or a real tool failure still fails', async () => {
});

test('macOS rows measured 2026-11-05 on run-7575: login items on Health, pf control, the helper own CPU wording, thermal not measured', async () => {
  const mac = (id, text) => readingStep(by(id), 'macos', driver(text));
  // login items: Health > Start-up items; the "need administrator rights" notice with the helper On is the product gap (startup.rs:178)
  assert.equal((await mac('login-items', 'Health Start-up items Programs that start at sign-in Not shown: Login items (System Settings > Login Items & Extensions) need administrator rights to read. Turn on administrator readings in Settings and Dyno helper will show them.')).status, 'fail');
  assert.equal((await mac('login-items', 'Health Start-up items Programs that start at sign-in Dyno Login item Showing 3 of 3 entries')).status, 'pass');
  // firewall: the control loads a known rule, so "0 rules" or a missing 59999 is a fail and a rule is a pass; a control that cannot run is check-broken
  assert.equal(by('firewall').prepare.macos, 'pf-rule');
  assert.equal((await mac('firewall', 'macOS packet filter (pf) Read by the administrator helper 0 rules. No rules to show.')).status, 'fail');
  assert.equal((await mac('firewall', 'macOS packet filter (pf) Read by the administrator helper 2 rules. Both All networks Ports = 59999')).status, 'pass');
  assert.equal((await readingStep(by('firewall'), 'macos', { ...driver('x'), prepare: async () => ({ ok: false, detail: 'pfctl missing' }) })).status, 'check-broken');
  // energy: the product's own sentence
  assert.equal((await mac('energy-and-thermal', 'This computer does not report CPU power (powermetrics listed none, as in a virtual machine) to software.')).status, 'pass');
  // thermal: the control adds a Temperature widget; its sentence is the pass, a dashboard without it fails
  assert.equal(by('thermal-pressure').prepare.macos, 'temp-widget');
  assert.equal((await mac('thermal-pressure', 'Temperature — Not available. No temperature sensor was reported. macOS reports a thermal pressure level, not temperatures, to software.')).status, 'pass');
  assert.equal((await mac('thermal-pressure', 'Dashboard Health CPU Memory')).status, 'fail');
});
test('macDriver pf-rule control loads the rule and checks the OS lists it; cleanup restores pf.conf only if it loaded one', async () => {
  const { macDriver } = await import('./helper-readings.mjs');
  const cmds = []; let listed = '1';
  const gui = { g: { exec: async (c) => { cmds.push(c); return { code: 0, stdout: listed, stderr: '' }; } }, closeSettings: async () => {} };
  const d = macDriver(gui);
  assert.equal((await d.prepare('pf-rule')).ok, true);
  assert.match(cmds[0], /pfctl -f - .*port 59999|port 59999.*pfctl -f -/s);
  await d.cleanup('pf-rule');
  assert.match(cmds[1], /pfctl -f \/etc\/pf\.conf/);
  listed = '0'; cmds.length = 0;
  assert.equal((await d.prepare('pf-rule')).ok, false);
  await d.cleanup('pf-rule');
  assert.equal(cmds.length, 1, 'nothing to restore when no rule was loaded');
});

test('macDriver text polls until the row\'s own words are on the page, so a reading that arrives seconds after the click is not judged early', async () => {
  const { macDriver } = await import('./helper-readings.mjs');
  let t = 0, reads = 0;
  const gui = { waitFor: async (fn, ms) => { for (; t <= ms; t += 3000) { const v = await fn(); if (v) return { ok: true, value: v }; } return { ok: false, value: null }; },
    texts: async () => [{ text: ++reads < 3 ? 'packet filter (pf) 0 rules.' : 'packet filter (pf) Read by the administrator helper 2 rules. Ports = 59999' }], closeSettings: async () => {} };
  const text = await macDriver(gui).text(by('firewall'));
  assert.match(text, /2 rules/);
  reads = -1000; t = 0;
  assert.match(await macDriver(gui).text(by('firewall')), /packet filter \(pf\) 0 rules/i);
});

test('temp-widget control: the new widget is the second CPU menu and the metric is checked; failures name the missing control and still leave the editor', async () => {
  const { addTemperatureWidget, removeTemperatureWidget } = await import('./helper-readings.mjs');
  const calls = []; let metric = 'CPU';
  const mk = (missing) => ({ app: 'Dyno', closeSettings: async () => {}, has: async () => true,
    press: async (a, re, role, nth) => { calls.push(`${re}${nth ? '#' + nth : ''}`); if (missing && re.includes(missing)) return false; if (/Temperature\$/.test(re) && role === 'AXStaticText') metric = 'Temperature'; return true; },
    waitFor: async (fn) => { const v = await fn(); return { ok: Boolean(v), value: v }; },
    texts: async () => [{ text: 'Widget options' }, { text: metric }] });
  assert.equal((await addTemperatureWidget(mk(null))).ok, true);
  assert.ok(calls.includes('^Actions for CPU$#1'), 'the second CPU menu is the new widget');
  const bad = await addTemperatureWidget(mk('Add Big number'));
  assert.equal(bad.ok, false); assert.match(bad.detail, /no "Add Big number" button/);
  calls.length = 0; await removeTemperatureWidget(mk(null));
  assert.deepEqual(calls.slice(0, 3), ['^Edit layout$', '^Actions for Temperature$', '^Remove Temperature$']);
});

test('the removed Backup health reading is reported as not applicable (removed in 1.1.0), is not driven and is not a required row', async () => {
  const fns = readingSteps('windows', async () => { throw new Error('no driver may be made for a removed row'); });
  const steps = [];
  for (const fn of fns.slice(-1)) steps.push(await fn({}));
  assert.equal(steps[0].status, 'not-applicable');
  assert.match(steps[0].summary, /removed in 1\.1\.0/);
  assert.ok(!READINGS.some((r) => /backup/i.test(r.id)));
});

// Linux "Processes of every account" (2026-11-08): root shows only in the Account column, which pd-table drops below about 750 px. A page without the column is a
// broken check (the probe could not see the thing, ER-33), never a product fail; the Linux driver makes room (minWidth) and polls instead of a fixed delay.
test('processes: no Account column is check-broken; Account then root passes on Linux', async () => {
  const narrow = await readingStep(by('processes-all-accounts'), 'linux', driver('Processes 212 processes Name CPU Memory dyno-agent Read by the administrator helper'));
  assert.equal(narrow.status, 'check-broken');
  assert.match(narrow.summary, /Account column not shown at this width/);
  assert.equal((await readingStep(by('processes-all-accounts'), 'linux', driver('Processes Name Account CPU systemd root 0.1 % Read by the administrator helper'))).status, 'pass');
  // "root" elsewhere on the page (a path) before the column header is not the reading
  assert.equal((await readingStep(by('processes-all-accounts'), 'linux', driver('/root/x Processes Name Account CPU systemd admin Read by the administrator helper'))).status, 'fail');
});
test('linux driver: honours minWidth (collapses the sidebar, resizes) and polls until the expectations show, bounded', async () => {
  const pressed = [];
  let reads = 0;
  let t = 0;
  const texts = ['Processes Loading', 'Processes Name Account', 'Processes Name Account systemd root Read by the administrator helper'];
  const d = linuxDriver({ press: (re) => { pressed.push(re); return true; }, tree: () => texts[Math.min(reads++, texts.length - 1)], resize: (w) => pressed.push(`resize ${w}`), sleep: async (ms) => { t += ms; }, now: () => t });
  assert.equal(await d.openPage('processes', null, by('processes-all-accounts')), true);
  assert.ok(pressed.includes('^Collapse sidebar'), pressed.join(','));
  assert.ok(pressed.some((p) => /^resize 1[12]\d\d$/.test(p)), pressed.join(','));
  const text = await d.text(by('processes-all-accounts'));
  assert.match(text, /root/);
  assert.equal(reads, 3);
  // never longer than 30 s when the text never comes
  t = 0; reads = 0;
  const d2 = linuxDriver({ press: () => true, tree: () => 'Processes', resize: () => {}, sleep: async (ms) => { t += ms; }, now: () => t });
  await d2.text(by('processes-all-accounts'));
  assert.ok(t <= 33_000 && t >= 30_000, `waited ${t}`);
});
