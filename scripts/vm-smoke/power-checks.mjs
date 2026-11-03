// The power release checks of Dyno 1.1.0 (apps/dyno/docs/releasing.md step 3b; docs: README.md "Power checks"): keep awake, scheduled
// wake and Wake-on-LAN, against a `guest` (the Tart VM from run.mjs, a fake in power-checks.test.mjs). Dyno's own CLI (`dyno awake`,
// `dyno wake-schedule`, `dyno wake`) is the actuator; the verdict always comes from what the OS says (power-probes.mjs). Every wait is
// a poll on state with a guard. The VM cannot sleep, so no check sleeps it: the schedule and hold are read from the OS tables.
import { dynoBlocksLinux, dynoHoldsMac, dynoRequestsWindows, minutesBetween, parseMagicPacket, parsePmsetAssertions, parsePmsetSched, parsePowercfgRequests, parseSleepStatesWindows, parseSystemdInhibit, parseWakeTimers } from './power-probes.mjs';

const sq = (s) => `'${String(s).replace(/'/g, `'\\''`)}'`;
export const TEST_MAC = '02:aa:bb:cc:dd:ee'; // a locally administered address nobody owns: the packet goes nowhere

/** Per-OS: how to call the CLI and read the OS's own tables. `read*` return { held|entries, evidence } from one guest command. */
export const ADAPTERS = {
  macos: {
    id: 'macos',
    cli: (bundle) => sq(`${bundle}/Contents/MacOS/dyno`),
    async readHold(g) {
      const r = await g.exec('pmset -g assertions');
      const held = dynoHoldsMac(parsePmsetAssertions(r.stdout));
      return { held: held.length > 0, evidence: [`$ pmset -g assertions`, ...held.map((a) => `  ${a.process} (pid ${a.pid}) ${a.type} "${a.name}" held ${a.held}`), held.length ? '' : '  (no Dyno assertion)'].filter(Boolean) };
    },
    async readWakes(g) {
      const r = await g.exec('pmset -g sched');
      const entries = parsePmsetSched(r.stdout).filter((e) => /dyno/i.test(e.owner));
      return { entries, evidence: ['$ pmset -g sched', ...(r.stdout.trim() ? r.stdout.trimEnd().split('\n') : ['(empty)'])] };
    },
    wakeNeedsHelper: true, // pmset schedule needs root: Dyno routes it through its admin helper
    async capture(g, { file }) {
      const iface = (await g.exec(`route -n get default 2>/dev/null | awk '/interface:/{print $2}'`)).stdout.trim() || 'en0';
      return { start: `sudo sh -c ${sq(`(tcpdump -l -n -xx -c 1 -i ${iface} 'udp and (port 9 or port 7)' > ${file} 2>&1 & echo $!)`)}`, ready: `grep -q 'listening on' ${sq(file)}`, iface };
    },
  },
  linux: {
    id: 'linux',
    cli: (bundle) => sq(`${bundle}/dyno`),
    async readHold(g) {
      const r = await g.exec('systemd-inhibit --list --no-pager');
      const held = dynoBlocksLinux(parseSystemdInhibit(r.stdout));
      return { held: held.length > 0, evidence: ['$ systemd-inhibit --list', ...held.map((h) => `  ${h.who} (${h.comm}) what=${h.what} why="${h.why}" mode=${h.mode}`), held.length ? '' : '  (no Dyno inhibitor)'].filter(Boolean) };
    },
    async readWakes(g) {
      // Dyno's route on Linux is its helper (rtcwake / a systemd timer with WakeSystem=); both are read.
      const r = await g.exec(`systemctl list-timers --all --no-pager 2>&1; echo ---; cat /sys/class/rtc/rtc0/wakealarm 2>/dev/null`);
      const entries = r.stdout.split('\n').filter((l) => /dyno/i.test(l)).map((l) => ({ owner: l.trim() }));
      return { entries, evidence: ['$ systemctl list-timers --all; wakealarm', ...r.stdout.trimEnd().split('\n').slice(0, 30)] };
    },
    wakeNeedsHelper: true,
    async capture(g, { file }) {
      return { start: `sudo sh -c ${sq(`(tcpdump -l -n -xx -c 1 -i any 'udp and (port 9 or port 7)' > ${file} 2>&1 & echo $!)`)}`, ready: `grep -q 'listening on' ${sq(file)}`, iface: 'any' };
    },
  },
  windows: {
    id: 'windows',
    cli: (bundle) => `& '${bundle}\\dyno.exe'`,
    async readHold(g) {
      const r = await g.exec('powercfg /requests');
      const held = dynoRequestsWindows(parsePowercfgRequests(r.stdout));
      return { held: held.length > 0, evidence: ['> powercfg /requests', ...held.map((h) => `  ${h.section}: ${h.kind} ${h.target} "${h.reason}"`), held.length ? '' : '  (no Dyno request)'].filter(Boolean) };
    },
    async readWakes(g) {
      const t = await g.exec('powercfg /waketimers');
      const a = await g.exec('powercfg /a');
      const entries = parseWakeTimers(t.stdout).filter((e) => /dyno/i.test(`${e.owner} ${e.reason}`));
      const states = parseSleepStatesWindows(a.stdout);
      return { entries, noSleepStates: states.length === 0, evidence: ['> powercfg /waketimers', ...t.stdout.trimEnd().split(/\r?\n/), '> powercfg /a (sleep states the guest has)', ...(states.length ? states : ['  (none: the VM has no sleep state, so Windows lists no wake timers)'])] };
    },
    wakeNeedsHelper: false,
    capture: null, // no tcpdump in the Windows guest; the capture is not wired there yet (README: not covered)
  },
};

const sleep = (g, ms) => g.sleep(ms);

/** Polls `fn` until it returns a truthy value or `guardMs` of the guest's own clock passes. */
async function until(g, fn, guardMs, intervalMs = 1500) {
  const t0 = g.now();
  for (;;) {
    const v = await fn();
    if (v) return { ok: true, value: v };
    if (g.now() - t0 >= guardMs) return { ok: false, value: v };
    await sleep(g, intervalMs);
  }
}

export class PowerChecks {
  /** @param guest {exec(cmd), now(), sleep(ms)}  @param os 'macos'|'linux'|'windows'  @param bundle where Dyno's `dyno` command is */
  constructor(guest, os, { bundle, log = () => {}, intervalMs = 1500 } = {}) {
    this.g = guest;
    this.os = ADAPTERS[os];
    if (!this.os) throw new Error(`no power adapter for ${os}`);
    this.bundle = bundle;
    this.log = log;
    this.interval = intervalMs;
    this.cli = this.os.cli(bundle);
  }

  dyno(args, timeoutMs = 60_000) { return this.g.exec(`${this.cli} ${args} 2>&1`, { timeoutMs }); }

  /** Keep awake for 1 hour: the OS must show Dyno's hold; off releases it. */
  async keepAwake() {
    const title = 'Keep awake: held by the OS, then released';
    const checks = [];
    const evidence = [];
    const before = await this.os.readHold(this.g);
    checks.push({ ok: !before.held, detail: before.held ? 'a Dyno hold was already there before "on" (dirty guest)' : 'no Dyno hold before "on"' });
    const on = await this.dyno('awake on --for 1h');
    evidence.push(`$ dyno awake on --for 1h (exit ${on.code})`, ...on.stdout.trimEnd().split('\n'));
    checks.push({ ok: on.code === 0, detail: `dyno awake on --for 1h exit ${on.code}` });
    const held = await until(this.g, async () => { const h = await this.os.readHold(this.g); return h.held ? h : null; }, 20_000, this.interval);
    const held_ev = held.ok ? held.value : await this.os.readHold(this.g);
    evidence.push(...held_ev.evidence);
    checks.push({ ok: held.ok, detail: held.ok ? 'the OS lists a Dyno hold that blocks sleep' : 'the OS never listed a Dyno hold within 20 s' });
    const status = await this.dyno('awake status');
    evidence.push(`$ dyno awake status (exit ${status.code})`, ...status.stdout.trimEnd().split('\n'));
    const off = await this.dyno('awake off');
    evidence.push(`$ dyno awake off (exit ${off.code})`, ...off.stdout.trimEnd().split('\n'));
    checks.push({ ok: off.code === 0, detail: `dyno awake off exit ${off.code}` });
    const gone = await until(this.g, async () => !(await this.os.readHold(this.g)).held, 20_000, this.interval);
    evidence.push(...(await this.os.readHold(this.g)).evidence);
    checks.push({ ok: gone.ok, detail: gone.ok ? 'the OS no longer lists a Dyno hold after "off"' : 'the Dyno hold was still listed 20 s after "off"' });
    return this.result(title, checks, evidence, 'held 1 h by the OS, released after off');
  }

  /** A wake 5 minutes ahead: the OS must list it; cancelling removes it. */
  async scheduledWake() {
    const title = 'Scheduled wake: listed by the OS, then removed';
    const checks = [];
    const evidence = [];
    const clock = await this.g.exec(this.os.id === 'windows' ? `(Get-Date).AddMinutes(5).ToString('HH:mm')` : `date -d '+5 minutes' +%H:%M 2>/dev/null || date -v+5M +%H:%M`);
    const at = clock.stdout.trim().split('\n').at(-1);
    if (!/^\d\d:\d\d$/.test(at)) return { title, status: 'fail', summary: `could not read the guest clock (${at})`, checks: [], evidence: [clock.stderr] };
    const name = 'vm-check';
    const add = await this.dyno(`wake-schedule add ${name} --at ${at} --repeat once --stay 10m`);
    evidence.push(`$ dyno wake-schedule add ${name} --at ${at} --repeat once --stay 10m (exit ${add.code})`, ...add.stdout.trimEnd().split('\n'));
    if (add.code !== 0 && /helper/i.test(add.stdout) && this.os.wakeNeedsHelper) {
      return { title, status: 'skip', summary: 'Dyno needs the admin helper to set a wake on this OS and it is not on: run the admin helper check first (it is a GUI row, see README)', checks, evidence };
    }
    checks.push({ ok: add.code === 0, detail: `wake-schedule add exit ${add.code}` });
    const listed = await until(this.g, async () => { const w = await this.os.readWakes(this.g); return w.entries.length ? w : null; }, 20_000, this.interval);
    const w = listed.ok ? listed.value : await this.os.readWakes(this.g);
    evidence.push(...w.evidence);
    if (!listed.ok && w.noSleepStates) {
      await this.dyno(`wake-schedule cancel ${name}`);
      return { title, status: 'skip', summary: 'the guest has no sleep state, so the OS lists no wake timers (known for the Windows VM); the schedule was accepted and cancelled', checks, evidence };
    }
    checks.push({ ok: listed.ok, detail: listed.ok ? `the OS lists ${w.entries.length} Dyno wake` : 'the OS listed no Dyno wake within 20 s' });
    const when = w.entries.find((e) => e.at);
    if (when) {
      const now = await this.g.exec(this.os.id === 'macos' ? `date '+%Y %m %d %H %M'` : `date '+%Y %m %d %H %M'`);
      const [y, mo, d, h, mi] = now.stdout.trim().split(/\s+/).map(Number);
      const delta = minutesBetween({ year: y, month: mo, day: d, hour: h, minute: mi }, when.at);
      checks.push({ ok: delta >= 3 && delta <= 7, detail: `the listed wake is ${delta} min ahead (expected about 5)` });
    }
    const cancel = await this.dyno(`wake-schedule cancel ${name}`);
    evidence.push(`$ dyno wake-schedule cancel ${name} (exit ${cancel.code})`, ...cancel.stdout.trimEnd().split('\n'));
    checks.push({ ok: cancel.code === 0, detail: `wake-schedule cancel exit ${cancel.code}` });
    const gone = await until(this.g, async () => (await this.os.readWakes(this.g)).entries.length === 0, 20_000, this.interval);
    checks.push({ ok: gone.ok, detail: gone.ok ? 'the OS no longer lists the wake' : 'the wake was still listed 20 s after cancel' });
    return this.result(title, checks, evidence, 'listed about 5 min ahead, gone after cancel');
  }

  /** Wake-on-LAN: the magic packet is captured on the wire and checked for the MAC and the UDP port. */
  async wakeOnLan() {
    const title = 'Wake-on-LAN: magic packet on the wire';
    if (!this.os.capture) return { title, status: 'skip', summary: `no packet capture is wired for ${this.os.id} guests (no tcpdump in the guest); run it from a macOS or Linux guest`, checks: [] };
    const file = '/tmp/vmsmoke-wol.txt';
    await this.g.exec(`rm -f ${file}`);
    const cap = await this.os.capture(this.g, { file });
    const started = await this.g.exec(cap.start);
    const tpid = Number(started.stdout.trim().split('\n').at(-1));
    const checks = [];
    const evidence = [];
    let text = '';
    try {
      const ready = await until(this.g, async () => (await this.g.exec(cap.ready)).code === 0, 15_000, 500);
      checks.push({ ok: ready.ok, detail: ready.ok ? `tcpdump listening on ${cap.iface}` : 'tcpdump did not start listening within 15 s' });
      const send = await this.dyno(`wake ${TEST_MAC}`);
      evidence.push(`$ dyno wake ${TEST_MAC} (exit ${send.code})`, ...send.stdout.trimEnd().split('\n'));
      checks.push({ ok: send.code === 0, detail: `dyno wake exit ${send.code}` });
      const got = await until(this.g, async () => { const r = await this.g.exec(`cat ${file}`); return /UDP/.test(r.stdout) ? r.stdout : null; }, 15_000, 500);
      text = got.ok ? got.value : (await this.g.exec(`cat ${file}`)).stdout;
      checks.push({ ok: got.ok, detail: got.ok ? 'a UDP packet to port 9 or 7 was captured' : 'no UDP packet to port 9 or 7 within 15 s' });
    } finally {
      if (Number.isInteger(tpid) && tpid > 0) await this.g.exec(`ps -p ${tpid} -o command= | grep -q tcpdump && sudo kill ${tpid}`);
    }
    evidence.push('--- tcpdump', ...text.trimEnd().split('\n').slice(0, 14));
    const p = parseMagicPacket(text);
    checks.push({ ok: p.magic, detail: p.magic ? `payload is a magic packet (6 x FF, then the address 16 times) for ${p.mac}` : 'the UDP payload is not a magic packet' });
    checks.push({ ok: p.mac === TEST_MAC, detail: `target address ${p.mac ?? 'none'} (expected ${TEST_MAC})` });
    checks.push({ ok: p.dstPort === 9 || p.dstPort === 7, detail: `UDP port ${p.dstPort ?? 'none'} (expected 9 or 7)` });
    return this.result(title, checks, evidence, `magic packet for ${p.mac} to ${p.dstAddress}:${p.dstPort}`);
  }

  result(title, checks, evidence, passSummary) {
    const failed = checks.filter((c) => !c.ok);
    return { title, status: failed.length ? 'fail' : 'pass', summary: failed.length ? failed.map((c) => c.detail).join('; ').slice(0, 240) : passSummary, checks, evidence };
  }
}

/** The power sequence once the app is installed and running (the caller starts it). */
export async function runPower(power) {
  const steps = [];
  for (const run of [() => power.keepAwake(), () => power.scheduledWake(), () => power.wakeOnLan()]) {
    const s = await run();
    steps.push(s);
    power.log(`${s.status.toUpperCase()}  ${s.title}: ${s.summary}`);
  }
  return steps;
}

/** Rows this suite does not automate, printed in the result as MANUAL with the reason (never silently dropped). */
export const POWER_MANUAL = [
  { title: 'Admin helper on/off: the OS prompt (manual)', summary: 'The macOS password sheet, Windows UAC prompt and Linux polkit prompt need a GUI driver (axdrive on macOS, UI Automation on Windows, a GNOME VM on Linux) that is not in this suite yet; no Dyno build with the helper existed when the suite was written.' },
  { title: 'Idle action: countdown, Cancel, Snooze, "Audio is playing" (manual)', summary: 'Needs the Power page driven in the UI and, for audio, a sound device in the guest. The VM cannot sleep, so a real sleep is never exercised either.' },
  { title: 'Screenshots: tray, dashboard customize, dialogs at 100/150 %, Windows tray menu, network map, benchmark (manual)', summary: 'Needs the GUI drivers above and the paddock-e2e screenshot check against the installed app.' },
];

/**
 * Install the installer, start Dyno (judged like a cold start), run the power steps, quit. `check` is the vm-smoke Check of the guest,
 * `makePower` builds the PowerChecks for it. Returns { steps, version } like runAll.
 */
export async function runPowerSuite(check, makePower, { stagedDmg, manual = POWER_MANUAL }) {
  const steps = [];
  const push = (s) => { steps.push(s); check.log(`${s.status.toUpperCase()}  ${s.title}: ${s.summary}`); return s; };
  const install = push(await check.install(stagedDmg));
  if (install.status === 'fail') return { steps, version: check.version };
  const start = push(await check.startAndJudge({ title: 'Cold start for the power checks', pre: async () => { await check.g.exec('sudo purge'); } }));
  if (start.status !== 'fail') for (const s of await runPower(makePower())) steps.push(s);
  push(await check.quit('Quit after the power checks'));
  for (const m of manual) steps.push({ title: m.title, status: 'manual', summary: m.summary });
  return { steps, version: check.version };
}
