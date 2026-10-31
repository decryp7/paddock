// The pure core of `scripts/vm-smoke --gui`: parsers for what the GUI test image prints (the quarantine attribute, axdrive's window
// list and AX trees, `sfltool dumpbtm`, the app's own log) and the judges over them. No I/O, no clock; gui.mjs collects the text.

/**
 * `com.apple.quarantine` ("0183;6ae4c31a;Safari;<uuid>") to its parts. Flags are hex; 0x40 is set once the user approved the first
 * launch (measured: 0083/0183 before, 00c3/01c3 after Open Anyway). null when the attribute is absent or not that shape.
 */
export function parseQuarantine(text) {
  const m = /^([0-9a-fA-F]{4});([0-9a-fA-F]+);([^;]*);(.*)$/.exec(String(text ?? '').trim());
  if (!m) return null;
  const flags = Number.parseInt(m[1], 16);
  return { flags, agent: m[3], approved: (flags & 0x40) !== 0, raw: m[0] };
}

/** `axdrive windows` TSV (owner, pid, layer, title, "x,y,wxh") to records; layer 0 is an ordinary app window. */
export function parseWindows(text) {
  const out = [];
  for (const line of String(text ?? '').split('\n')) {
    const f = line.split('\t');
    if (f.length < 5) continue;
    const g = /^(-?[\d.]+),(-?[\d.]+),([\d.]+)x([\d.]+)$/.exec(f[4]);
    out.push({ owner: f[0], pid: Number(f[1]), layer: Number(f[2]), title: f[3], x: g ? Number(g[1]) : 0, y: g ? Number(g[2]) : 0, w: g ? Number(g[3]) : 0, h: g ? Number(g[4]) : 0 });
  }
  return out;
}

/** The app's ordinary windows (layer 0) big enough to be a window, not a menu-bar item or a tooltip. */
export function appWindows(windows, owner) {
  return windows.filter((w) => w.owner === owner && w.layer === 0 && w.w >= 200 && w.h >= 150);
}

/** `axdrive tree` text to { depth, role, label } lines (two spaces per level; role and label separated by two spaces). */
export function parseTree(text) {
  const out = [];
  for (const line of String(text ?? '').split('\n')) {
    const m = /^((?: {2})*)(AX\w+)(?: {2}(.*))?$/.exec(line.replace(/\s+$/, ''));
    if (m) out.push({ depth: m[1].length / 2, role: m[2], label: m[3] ?? '' });
  }
  return out;
}

/**
 * The rows of System Settings > General > Login Items & Extensions from its AX tree (macOS 26: an "Open at Login" outline, then
 * "App Background Activity" groups of a name, a detail text `… | background-detail-for-<name>-…` and a switch
 * `background-switch-<name> | value=0|1`).
 * @returns {{openAtLogin: string[], background: {name:string, detail:string, on:boolean|null}[]}}
 */
export function parseLoginItems(treeText) {
  const lines = parseTree(treeText);
  const openAt = lines.findIndex((l) => l.role === 'AXHeading' && /^Open at Login\b/.test(l.label));
  const bgAt = lines.findIndex((l) => l.role === 'AXHeading' && /^App Background Activity\b/.test(l.label));
  const openAtLogin = [];
  if (openAt >= 0) {
    const end = bgAt > openAt ? bgAt : lines.length;
    for (let i = openAt; i < end; i++) {
      if (lines[i].role !== 'AXRow') continue;
      const rowDepth = lines[i].depth;
      for (let j = i + 1; j < end && lines[j].depth > rowDepth; j++) {
        if (lines[j].role === 'AXStaticText' || lines[j].role === 'AXTextField') { openAtLogin.push(lines[j].label.split(' | ')[0]); break; }
      }
    }
  }
  const background = [];
  if (bgAt >= 0) {
    for (const l of lines.slice(bgAt)) {
      if (l.role === 'AXHeading' && l !== lines[bgAt]) break;
      const d = /^(.*) \| background-detail-for-(.+?)-(?:app-)?login-items$/.exec(l.label);
      if (l.role === 'AXStaticText' && d) background.push({ name: d[2], detail: d[1], on: null });
      const s = /(?:^| \| )background-switch-(.+?) \| value=([01])$/.exec(l.label);
      if (l.role === 'AXCheckBox' && s && !/^Show in Finder \|/.test(l.label)) {
        const row = background.find((b) => b.name === s[1] && b.on === null);
        if (row) row.on = s[2] === '1';
        else background.push({ name: s[1], detail: '', on: s[2] === '1' });
      }
    }
  }
  return { openAtLogin, background };
}

/**
 * The Login Items rows against what the app should have: exactly the expected "Open at Login" names, the expected background rows
 * present and switched on, and no other row that is not the image's own (an "Item from unidentified developer" of the app, such
 * as a legacy LaunchAgents plist, fails).
 * @param {{openAtLogin?: string[], background?: string[]}} want
 * @param {string[]} imageRows background rows the test image itself has (cirruslabs' agent, its maxfiles daemon)
 */
export function judgeLoginItems(rows, want, imageRows) {
  const checks = [];
  const expOpen = [...(want.openAtLogin ?? [])].sort();
  const gotOpen = [...rows.openAtLogin].sort();
  checks.push({ ok: JSON.stringify(expOpen) === JSON.stringify(gotOpen), detail: `Open at Login: ${gotOpen.length ? gotOpen.join(', ') : '(none)'} (expected ${expOpen.length ? expOpen.join(', ') : '(none)'})` });
  for (const name of want.background ?? []) {
    const r = rows.background.filter((b) => b.name === name);
    checks.push({ ok: r.length === 1 && r[0].on === true, detail: `App Background Activity "${name}": ${r.length === 0 ? 'missing' : r.length > 1 ? `${r.length} rows` : `${r[0].detail || 'present'}, switch ${r[0].on ? 'on' : 'off'}`}` });
  }
  const extra = rows.background.filter((b) => !(want.background ?? []).includes(b.name) && !imageRows.includes(b.name));
  checks.push({ ok: extra.length === 0, detail: extra.length ? `unexpected background row(s): ${extra.map((b) => `"${b.name}" (${b.detail || 'no detail'})`).join(', ')}` : 'no other background row besides the image\'s own' });
  return { ok: checks.every((c) => c.ok), checks };
}

/** `sfltool dumpbtm` to its item records ({ name, type, disposition, identifier, url, executable, parent, developer }). */
export function parseBtmDump(text) {
  const items = [];
  let cur = null;
  for (const line of String(text ?? '').split('\n')) {
    if (/^\s*#\d+:\s*$/.test(line)) { cur = {}; items.push(cur); continue; }
    const m = /^\s*([A-Za-z ]+?):\s(.*)$/.exec(line);
    if (!cur || !m) continue;
    const key = { Name: 'name', Type: 'type', Disposition: 'disposition', Identifier: 'identifier', URL: 'url', 'Executable Path': 'executable', 'Parent Identifier': 'parent', 'Developer Name': 'developer' }[m[1].trim()];
    if (key && cur[key] === undefined) cur[key] = m[2].trim();
  }
  return items;
}

/** The BTM record of a launchd label (Identifier `<n>.<label>`), with its disposition flags; null when absent. */
export function btmItem(items, label) {
  const it = items.find((i) => typeof i.identifier === 'string' && i.identifier.replace(/^\d+\./, '') === label);
  if (!it) return null;
  const d = /\[([^\]]*)\]/.exec(it.disposition ?? '')?.[1].split(',').map((s) => s.trim()) ?? [];
  return { ...it, enabled: d.includes('enabled'), allowed: d.includes('allowed'), notified: d.includes('notified') };
}

/**
 * How the app decided to start after the login: the app's log written since the reboot must have the hidden line and not the
 * window line (both regexes from the app's config), and no window of the app may be on screen.
 */
export function judgeLoginStart(logSinceReboot, windows, cfg) {
  const t = String(logSinceReboot ?? '');
  const hidden = new RegExp(cfg.hidden).exec(t);
  const shown = new RegExp(cfg.shown).exec(t);
  const wins = appWindows(windows, cfg.windowOwner);
  const checks = [
    { ok: !!hidden, detail: hidden ? `log since the reboot: "${hidden[0]}"` : `log since the reboot has no line matching /${cfg.hidden}/` },
    { ok: !shown, detail: shown ? `log since the reboot: "${shown[0]}" (the login start opened a window)` : `no /${cfg.shown}/ line since the reboot` },
    { ok: wins.length === 0, detail: wins.length ? `window(s) on screen after the login: ${wins.map((w) => `"${w.title}" ${w.w}x${w.h}`).join(', ')}` : 'no window of the app on screen after the login' },
  ];
  return { ok: checks.every((c) => c.ok), checks };
}
