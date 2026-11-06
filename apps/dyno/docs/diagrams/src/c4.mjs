import { P, save, link, hub } from './common.mjs';

/** Stack nodes in a column evenly over height H (each at least its fitted height). Returns the minimum height needed. */
const need = (d, w, list, gap = 40) => list.reduce((s, n) => s + d.fit(w, n[3]), 0) + gap * (list.length - 1);
function column(d, x, w, y0, H, list) {
  const hs = list.map((n) => d.fit(w, n[3]));
  const free = H - hs.reduce((a, b) => a + b, 0);
  const gap = list.length > 1 ? free / (list.length - 1) : 0;
  let y = y0;
  list.forEach(([id, role], i) => { d.node(id, x, Math.round(y), w, hs[i], role, list[i][3]); y += hs[i] + gap; });
}

// ---------- C4 level 1 ----------
{
  const d = P({ title: 'Dyno — system context', kind: 'C4 · Level 1 — System context', w: 1344,
    desc: 'Dyno measures and explains one PC for its owner. It also keeps the computer awake, wakes other computers, and sleeps, hibernates or shuts down after idle or at a set time, as the owner sets it up. Sister apps and the release host are reached only on a click or an opt-in.' });
  const W = 288, y0 = 144;
  const left = [
    ['owner', 'process', 0, { name: 'PC owner', tech: 'Person', desc: 'Watches the machine, asks why it is slow, gets alarms, runs benchmarks on a click.', person: true }],
    ['data', 'process', 0, { name: 'Local data', tech: 'SQLite + JSON in the user data folder', desc: 'History, settings, alarms and notes, snapshots, baselines. Never uploaded.' }],
    ['os', 'process', 0, { name: 'Operating system', tech: 'Windows · macOS · Linux', desc: 'Counters, process table, /proc and /sys, OS logs, launchd, systemd, registry. Figures it shows only to administrators are marked Unavailable.' }],
  ];
  const right = [
    ['hosts', 'external', 0, { name: 'Release host', tech: 'Gitea releases, HTTPS', desc: 'Signed installers and latest.json for Dyno\'s own opt-in updates.' }],
    ['debrief', 'external', 0, { name: 'Debrief', tech: 'Sister app', desc: 'Open in Debrief: a time window exported for log analysis.', planned: true }],
    ['mcp', 'external', 0, { name: 'Local MCP client', tech: 'AI app on this PC', desc: 'Reads Dyno once AI access is on; wakes a saved computer or keeps this one awake only if allowed.' }],
    ['pitwall', 'external', 0, { name: 'Pitwall', tech: 'Local AI hub', desc: 'Planned for 1.1 as an MCP client of Dyno.', planned: true }],
    ['lan', 'external', 0, { name: 'Computers on the LAN', tech: 'Wake-on-LAN', desc: 'Other computers the owner saved; Dyno wakes one when asked.' }],
  ];
  const H = Math.max(need(d, W, left), need(d, W, right));
  column(d, 48, W, y0, H, left);
  column(d, 1008, W, y0, H, right);
  d.node('sys', 528, y0, W, H, 'accent', { name: 'Dyno', tech: 'Tauri app + per-user agent + Rust core',
    desc: 'A per-user agent that samples, keeps history and raises alarms; a Tauri app (Angular UI) and the dyno command that show it; a pure Rust core that decides. It runs with the user\'s rights; an optional administrator helper does what needs those rights.' });
  const [, oy, , oh] = d.rect.os;
  const hd = { name: 'dyno-helper', tech: 'Optional OS service', desc: 'Turned on in Settings. Reads what needs administrator rights, sets Dyno\'s own scheduled wakes (macOS, Linux) and installs the pinned disk-health tool on request (Windows), with an audit log.' };
  const hh = d.fit(240, hd);
  const ht = oy + oh - 12 - hh, at = (f) => (oy + oh * f - ht) / hh;
  d.node('helper', 552, ht, 240, hh, 'accent', hd);
  hub(d, 'sys', 'L', 'owner', 'R', { rev: true, label: 'Views, asks' });
  link(d, 'sys', 'L', 'os', 'R', { label: 'Reads, asks to sleep', t: [(oy + oh * 0.15 - y0) / H, 0.15] });
  link(d, 'helper', 'L', 'os', 'R', { label: 'Reads as admin', t: [at(0.5), 0.5] });
  link(d, 'helper', 'L', 'os', 'R', { label: 'Sets wakes', t: [at(0.82), 0.82] });
  hub(d, 'sys', 'L', 'data', 'R', { label: 'Reads, writes' });
  hub(d, 'sys', 'R', 'hosts', 'L', { label: 'Checks', dash: true });
  hub(d, 'sys', 'R', 'debrief', 'L', { label: 'Exports', dash: true });
  hub(d, 'sys', 'R', 'mcp', 'L', { rev: true, label: 'Reads, asks', dash: true });
  hub(d, 'sys', 'R', 'pitwall', 'L', { rev: true, label: 'Reads', dash: true });
  hub(d, 'sys', 'R', 'lan', 'L', { label: 'Wakes', dash: true });
  d.legend([{ kind: 'process', label: 'Person · OS · data' }, { kind: 'accent', label: 'Dyno' }, { kind: 'external', label: 'External system' }, { kind: 'tag', label: 'Planned', role: 'external' }, { kind: 'line', label: 'Uses' }]);
  save('c4-1-context', d, y0 + H);
}

// ---------- C4 level 2 ----------
{
  const d = P({ title: 'Dyno — containers', kind: 'C4 · Level 2 — Containers', w: 1496,
    desc: 'The app and the agent run as the user; the optional helper is an administrator service. Plugins and benchmarks are separate processes. Dyno\'s processes talk over paddock-ipc.' });
  const NH = 120, LW = 232, aw = 256, bw = 288, gap = 128;
  const lx = 48, gx = lx + LW + 96, ax = gx + 24, bx = ax + aw + gap, gw = 24 + aw + gap + bw + 24, rx = gx + gw + 80;
  const r = (g, i) => g + 40 + i * (NH + 32);
  const g1 = 144, h1 = 40 + NH + 24, g2 = g1 + h1 + 56, h2 = 40 + NH * 3 + 64 + 24;
  d.node('person', lx, r(g1, 0), LW, NH, 'process', { name: 'PC owner', tech: 'Person', desc: 'Window and tray.', person: true });
  d.group('g1', gx, g1, gw, h1, 'App process [Tauri 2 · user session]');
  d.node('ui', ax, r(g1, 0), aw, NH, 'ui', { name: 'Angular UI', tech: 'Angular 22, signals, ECharts', desc: 'Pages and SignalStores behind the BACKEND token; never calls invoke.' });
  d.node('shell', bx, r(g1, 0), bw, NH, 'accent', { name: 'Tauri shell', tech: 'Rust, app/src-tauri', desc: 'Commands, tray, windows; parses UI JSON into Requests; serves MCP while open.' });
  d.node('upd', rx, r(g1, 0), aw, NH, 'external', { name: 'Release host', tech: 'HTTPS', desc: 'latest.json and signed installers; opt-in.' });
  d.group('g2', gx, g2, gw, h2, 'Agent process [user session, started at login]', { os: true });
  d.node('core', ax, r(g2, 0), aw, NH, 'core', { name: 'dyno-core', tech: 'Rust library, pure', desc: 'Model, grouping, diagnosis, alarms, health, protocol. No I/O, no clock.' });
  d.node('platform', ax, r(g2, 1), aw, NH, 'accent', { name: 'dyno-platform', tech: 'Rust library', desc: 'Adapters per OS behind the core\'s ports, plus fakes for tests.' });
  d.node('db', ax, r(g2, 2), aw, NH, 'process', { name: 'Data folder', tech: 'SQLite dyno.db + JSON', desc: 'History, alarms, snapshots; settings, agent.json.' });
  d.node('agent', bx, r(g2, 0), bw, NH * 3 + 64, 'accent', { name: 'dyno-agent', tech: 'Rust binary', desc: 'Sampler, ipc server, request handlers, alarms, issues, power actions, flight recorder, history writer. The single writer of Dyno\'s data.' });
  d.node('os', lx, r(g2, 1), LW, NH, 'process', { name: 'Operating system', tech: 'Windows · macOS · Linux', desc: 'Counters, /proc, Mach, OS logs, launchd, systemd.' });
  d.node('plugins', rx, r(g2, 0), aw, NH, 'plugin', { name: 'Plugins', tech: 'Rust, out of process', desc: 'Vendor sensors. Depend on dyno-plugin-api only; guarded by dyno-plugin-host.' });
  d.node('bench', rx, r(g2, 1), aw, NH, 'process', { name: 'dyno-bench', tech: 'Rust binary, child process', desc: 'CPU, memory and disk benchmarks, one at a time, killed on timeout.' });
  d.node('helper', rx, r(g2, 2), aw, NH, 'accent', { name: 'dyno-helper', tech: 'Rust binary, OS service', desc: 'Optional, as administrator: readings, scheduled wakes, the Windows disk-health tool; audit log.' });
  d.node('cli', bx, g2 + h2 + 56, bw, 112, 'accent', { name: 'dyno', tech: 'Rust binary, command line', desc: 'Readings, plus awake, idle, wake, wake-schedule; mcp bridges to the app\'s MCP listener.' });
  link(d, 'person', 'R', 'ui', 'L', { label: 'Uses' });
  link(d, 'ui', 'R', 'shell', 'L', { label: 'Commands' });
  link(d, 'shell', 'R', 'upd', 'L', { label: 'Checks', dash: true });
  link(d, 'shell', 'B', 'agent', 'T', { label: 'Requests, pushes' });
  hub(d, 'agent', 'L', 'core', 'R', { label: 'Decisions' });
  link(d, 'cli', 'T', 'agent', 'B', { label: 'Requests' });
  link(d, 'platform', 'T', 'core', 'B', { label: 'Implements', dash: true, arrow: 'open' });
  link(d, 'platform', 'L', 'os', 'R', { label: 'Reads' });
  hub(d, 'agent', 'L', 'db', 'R', { label: 'Reads, writes' });
  hub(d, 'agent', 'R', 'plugins', 'L', { label: 'Calls' });
  hub(d, 'agent', 'R', 'bench', 'L', { label: 'Spawns' });
  hub(d, 'agent', 'R', 'helper', 'L', { label: 'Asks', dash: true });
  d.legend([{ kind: 'ui', label: 'Angular UI' }, { kind: 'accent', label: 'Dyno container' }, { kind: 'core', label: 'Pure core' }, { kind: 'plugin', label: 'Plugin' }, { kind: 'process', label: 'Process · OS · data' }, { kind: 'external', label: 'External' }, { kind: 'line', label: 'Uses' }, { kind: 'dashline', label: 'Optional · implements' }]);
  save('c4-2-containers', d, g2 + h2 + 56 + 112);
}
