// Pure parsers for the power release checks (apps/dyno/docs/releasing.md step 3b; docs: README.md). They read what the OS itself says
// (pmset, powercfg, systemd-inhibit, tcpdump), so a check is judged by the OS's state and never by what Dyno claims. No I/O here.

/** `pmset -g assertions`: the per-process lines. Returns [{ pid, process, held, type, name }]. */
export function parsePmsetAssertions(text) {
  const out = [];
  for (const line of String(text).split('\n')) {
    const m = /^\s*pid (\d+)\(([^)]*)\):\s+\[0x[0-9a-fA-F]+\]\s+(\d+:\d+:\d+)\s+(\w+)\s+named:\s+"(.*)"/.exec(line);
    if (m) out.push({ pid: Number(m[1]), process: m[2], held: m[3], type: m[4], name: m[5] });
  }
  return out;
}

const SLEEP_BLOCKING_MAC = new Set(['PreventUserIdleSystemSleep', 'PreventSystemSleep', 'NoIdleSleepAssertion', 'PreventUserIdleDisplaySleep']);

/** The assertions that hold the Mac awake and belong to a Dyno process. */
export function dynoHoldsMac(assertions) {
  return assertions.filter((a) => /^dyno/i.test(a.process) && SLEEP_BLOCKING_MAC.has(a.type));
}

/** `pmset -g sched`: one-off events ("[0] wake at 11/04/2026 06:10:00 by 'owner'"). Returns [{ index, kind, at, owner }]. */
export function parsePmsetSched(text) {
  const out = [];
  for (const line of String(text).split('\n')) {
    const m = /^\s*\[(\d+)\]\s+(\w+)\s+at\s+(\d\d)\/(\d\d)\/(\d{4})\s+(\d\d):(\d\d):(\d\d)\s+by\s+'([^']*)'/.exec(line);
    if (m) out.push({ index: Number(m[1]), kind: m[2], at: { month: +m[3], day: +m[4], year: +m[5], hour: +m[6], minute: +m[7], second: +m[8] }, owner: m[9] });
  }
  return out;
}

/** Minutes between two local wall-clock moments given as parsePmsetSched's `at` and a Date-like {year,month,day,hour,minute}. */
export function minutesBetween(a, b) {
  const t = (x) => Date.UTC(x.year, x.month - 1, x.day, x.hour, x.minute, x.second ?? 0) / 60000;
  return t(b) - t(a);
}

/** `powercfg /requests`: { DISPLAY: [{ kind, target, reason }], SYSTEM: [...], AWAYMODE: [...] }. */
export function parsePowercfgRequests(text) {
  const sections = {};
  let cur = null;
  let item = null;
  for (const raw of String(text).split(/\r?\n/)) {
    const line = raw.trim();
    const h = /^([A-Z]+):$/.exec(line);
    if (h) { cur = sections[h[1]] = []; item = null; continue; }
    if (!cur || !line || /^none\.?$/i.test(line)) continue;
    const r = /^\[(PROCESS|DRIVER|SERVICE|LEGACY KERNEL CALLER)\]\s*(.*)$/i.exec(line);
    if (r) { item = { kind: r[1].toUpperCase(), target: r[2], reason: '' }; cur.push(item); } else if (item && !item.reason) item.reason = line;
  }
  return sections;
}

/** The SYSTEM or DISPLAY requests that belong to Dyno (its program path or its reason text names it). */
export function dynoRequestsWindows(sections) {
  return ['SYSTEM', 'DISPLAY', 'AWAYMODE'].flatMap((k) => (sections[k] ?? []).filter((i) => /dyno/i.test(`${i.target} ${i.reason}`)).map((i) => ({ ...i, section: k })));
}

/** `powercfg /waketimers`: [{ owner, expires }]; empty when it says there are none. */
export function parseWakeTimers(text) {
  const out = [];
  const lines = String(text).split(/\r?\n/);
  for (let i = 0; i < lines.length; i++) {
    const m = /^Timer set by\s+(.+?)\s+expires at\s+(.+?)\s+on\s+(.+?)\.?\s*$/.exec(lines[i].trim());
    if (m) out.push({ owner: m[1], expires: `${m[3]} ${m[2]}`, reason: (/^\s*Reason:\s*(.*)$/.exec(lines[i + 1] ?? '') ?? [])[1] ?? '' });
  }
  return out;
}

/** `powercfg /a`: the sleep states the machine has ("The following sleep states are available" lines before "not available"). */
export function parseSleepStatesWindows(text) {
  const m = /sleep states are available on this system:?\s*\r?\n([\s\S]*?)(?:\r?\n\s*\r?\n|$)/i.exec(String(text));
  if (!m) return [];
  return m[1].split(/\r?\n/).map((l) => l.trim()).filter((l) => /^(Standby|Hibernate|Hybrid Sleep|Fast Startup)/i.test(l));
}

/** `systemd-inhibit --list`: [{ who, what, why, mode }] from the whitespace table (WHO UID USER PID COMM WHAT WHY MODE). */
export function parseSystemdInhibit(text) {
  const out = [];
  const lines = String(text).split('\n').map((l) => l.trimEnd()).filter(Boolean);
  const head = lines.find((l) => /^WHO\s+UID/.test(l));
  if (!head) return out;
  const cols = ['WHO', 'UID', 'USER', 'PID', 'COMM', 'WHAT', 'WHY', 'MODE'].map((c) => head.indexOf(c));
  for (const l of lines.slice(lines.indexOf(head) + 1)) {
    if (/^\d+ inhibitors? listed/.test(l)) break;
    const cell = (i) => l.slice(cols[i], i + 1 < cols.length ? cols[i + 1] : undefined).trim();
    out.push({ who: cell(0), pid: cell(3), comm: cell(4), what: cell(5), why: cell(6), mode: cell(7) });
  }
  return out;
}

export function dynoBlocksLinux(rows) {
  return rows.filter((r) => /dyno/i.test(`${r.who} ${r.comm} ${r.why}`) && /sleep|idle/.test(r.what) && r.mode === 'block');
}

/**
 * One packet of `tcpdump -n -xx` text: { dstAddress, dstPort, mac, magic }. `magic` is true when the UDP payload holds six 0xFF bytes
 * then the target's hardware address 16 times (the Wake-on-LAN magic packet); `mac` is that address ("aa:bb:...").
 */
export function parseMagicPacket(text) {
  const header = /IP6?\s+\S+\s+>\s+([\d.:a-fA-F]+)\.(\d+):\s+UDP/.exec(text);
  const bytes = [];
  for (const l of String(text).split('\n')) {
    const m = /^\s*0x[0-9a-fA-F]{4}:\s+((?:[0-9a-fA-F]{2,4}\s?)+)/.exec(l);
    if (m) for (const g of m[1].trim().split(/\s+/)) for (let i = 0; i < g.length; i += 2) bytes.push(parseInt(g.slice(i, i + 2), 16));
  }
  let mac = null;
  for (let i = 0; i + 102 <= bytes.length && !mac; i++) {
    if (!bytes.slice(i, i + 6).every((b) => b === 0xff)) continue;
    const m = bytes.slice(i + 6, i + 12);
    if (m.every((b) => b === 0xff)) continue;
    let ok = true;
    for (let r = 1; r < 16 && ok; r++) ok = bytes.slice(i + 6 + r * 6, i + 12 + r * 6).every((b, k) => b === m[k]);
    if (ok) mac = m.map((b) => b.toString(16).padStart(2, '0')).join(':');
  }
  return { dstAddress: header?.[1] ?? null, dstPort: header ? Number(header[2]) : null, mac, magic: mac !== null };
}
