import { P, save, link, hub } from './common.mjs';
const Y = 144;

// ---------- agent: requests and sampling ----------
{
  const d = P({ title: 'dyno-agent — requests and sampling', kind: 'C4 · Level 3 — Components of the agent', w: 1344,
    desc: 'A sampling loop feeds history, alarms and the flight recorder; an ipc server with one thread per connection answers the app; one SQLite writer holds the data. Not shown: the helper link, power actions, wakes and Wake-on-LAN (1.1.0).' });
  const NH = 112, CW = 248, G = 120, ax = 48, gx = ax + 216 + 72, c0 = gx + 24, c1 = c0 + CW + G, c2 = c1 + CW + G;
  const r = (i) => Y + 40 + i * (NH + 32);
  const GH = 40 + NH * 4 + 32 * 3 + 24;
  d.node('app', ax, r(0), 216, NH, 'accent', { name: 'App', tech: 'Tauri shell, client', desc: 'Subscribes and sends requests.' });
  d.group('g', gx, Y, 24 + 3 * CW + 2 * G + 24, GH, 'dyno-agent [Rust crate]', { os: true });
  d.node('server', c0, r(0), CW, NH, 'accent', { name: 'server', tech: 'ipc host, thread per connection', desc: 'Binds the private endpoint; checks token and protocol; bounded push queue.' });
  d.node('handlers', c1, r(0), CW, NH, 'accent', { name: 'handlers', tech: 'module', desc: 'One function per Request: validated, answered from measured or stored data.' });
  d.node('sampler', c0, r(1), CW, NH * 3 + 64, 'accent', { name: 'sampler', tech: 'thread', desc: 'Ticks at adaptive intervals: sensors, processes, groups, diagnosis, baselines; pushes LiveUpdate; measures its own overhead.' });
  d.node('rollup', c1, r(1), CW, NH, 'accent', { name: 'rollup', tech: 'HistoryWriter', desc: 'Raw points, then minute, 15-minute and hour buckets with min, average, max.' });
  d.node('alarms', c1, r(2), CW, NH, 'accent', { name: 'alarms', tech: 'AlarmCentre', desc: 'Raises, acknowledges, shelves; pushes every change.' });
  d.node('rec', c1, r(3), CW, NH, 'accent', { name: 'recorder', tech: 'flight recorder', desc: 'Last ten minutes at one second in a bounded ring; saved on request or on a serious alarm.' });
  d.node('store', c2, r(0), CW, NH * 4 + 96, 'process', { name: 'store', tech: 'SQLite, rusqlite bundled', desc: 'Single writer; WAL; versioned schema; a damaged file is moved aside and a new one started.' });
  link(d, 'app', 'R', 'server', 'L', { label: 'Requests' });
  link(d, 'server', 'R', 'handlers', 'L', { label: 'Request' });
  link(d, 'sampler', 'T', 'server', 'B', { label: 'Push', dash: true });
  hub(d, 'sampler', 'R', 'rollup', 'L', { label: 'Records' });
  hub(d, 'sampler', 'R', 'alarms', 'L', { label: 'Alerts' });
  hub(d, 'sampler', 'R', 'rec', 'L', { label: 'Samples' });
  hub(d, 'store', 'L', 'handlers', 'R', { rev: true, label: 'Reads, writes' });
  hub(d, 'store', 'L', 'rollup', 'R', { rev: true, label: 'Puts points' });
  hub(d, 'store', 'L', 'alarms', 'R', { rev: true, label: 'Persists' });
  hub(d, 'store', 'L', 'rec', 'R', { rev: true, label: 'Snapshots' });
  d.legend([{ kind: 'accent', label: 'Agent module' }, { kind: 'process', label: 'Store' }, { kind: 'line', label: 'Calls' }, { kind: 'dashline', label: 'Push' }]);
  save('c4-3-components-agent', d, Y + GH);
}

// ---------- agent: support ----------
{
  const d = P({ title: 'dyno-agent — support modules', kind: 'C4 · Level 3 — Components of the agent', w: 1472,
    desc: 'What keeps the agent running and registered, its background readers, the plugin host and the benchmark runner. Every decision is a dyno-core call; every OS access goes through dyno-platform. Not shown: the helper link, power actions, wakes and Wake-on-LAN (1.1.0).' });
  const NH = 112, CW = 216, G = 104, ox = 48, gx = ox + 176 + 104, c0 = gx + 24, c1 = c0 + CW + G, c2 = c1 + CW + G, ex = c2 + CW + 24 + 72;
  const r = (i) => Y + 40 + i * (NH + 32);
  const BG = { name: 'issues · unusual · netwatch', tech: 'three threads', desc: 'OS logs every 15 min; programs against the baseline every 30 s (ADR 0017); Wi-Fi, VPN and IP changes on a slow poll.' };
  const VW = { name: 'counters · hardware · extras', tech: 'modules', desc: 'Counter browser, pinned counters every 15 s; throttling, disk and battery life, baselines (ADR 0018); read-only OS tools, boot records and Extras (ADR 0020).' };
  const bgH = Math.max(NH, d.fit(CW, BG)), vwH = Math.max(NH, d.fit(CW, VW)), vwY = r(2) + bgH + 32;
  const GH = vwY + vwH + 24 - Y;
  d.node('sm', ox, r(1), 176, NH, 'process', { name: 'Service manager', tech: 'launchd · systemd · Run', desc: 'Starts the agent at login.' });
  d.group('g', gx, Y, 24 + 3 * CW + 2 * G + 24, GH, 'dyno-agent [Rust crate]', { os: true });
  d.node('cli', c0, r(0), CW, NH, 'accent', { name: 'cli', tech: 'module', desc: 'run · status · stop · once · bench · service install, uninstall, status, ensure.' });
  d.node('service', c0, r(1), CW, NH, 'accent', { name: 'service', tech: 'module', desc: 'Launch agent, systemd unit, Run entry. The unit text is pure.' });
  d.node('agent', c1, r(0), CW, vwY + vwH - r(0), 'accent', { name: 'agent (Shared)', tech: 'module', desc: 'Shared state behind locks that recover from poisoning; subscribers; wake and stop.' });
  d.node('disc', c2, r(0), CW, NH, 'accent', { name: 'files · diskusage', tech: 'modules', desc: 'agent.json, the data folder, atomic JSON writes; the bounded disk map.' });
  d.node('bench', c2, r(1), CW, NH, 'accent', { name: 'bench', tech: 'BenchCentre', desc: 'On a click, one kind at a time, a timeout each.' });
  d.node('ph', c2, r(2), CW, NH, 'accent', { name: 'dyno-plugin-host', tech: 'crate', desc: 'Starts sensor plugins on first use; guards, quarantines.' });
  d.node('bg', c0, r(2), CW, bgH, 'accent', BG);
  d.node('views', c0, vwY, CW, vwH, 'accent', VW);
  d.node('bproc', ex, r(1), 216, NH, 'process', { name: 'dyno-bench', tech: 'child process', desc: 'CPU, memory and disk tests.' });
  d.node('plug', ex, r(2), 216, NH, 'plugin', { name: 'Plugins', tech: 'out of process', desc: 'Vendor sensors, over paddock-ipc.' });
  hub(d, 'agent', 'L', 'cli', 'R', { rev: true, label: 'Runs' });
  link(d, 'cli', 'B', 'service', 'T', { label: 'Install' });
  link(d, 'service', 'L', 'sm', 'R', {});
  d.lbl('Registers', (ox + 176 + gx) / 2, d.rect.service[1] + d.rect.service[3] / 2, 'a', { maxw: gx - ox - 176 - 8 });
  hub(d, 'agent', 'R', 'disc', 'L', { label: 'Writes' });
  hub(d, 'agent', 'R', 'bench', 'L', { label: 'Owns' });
  hub(d, 'agent', 'R', 'ph', 'L', { label: 'Owns' });
  link(d, 'bench', 'R', 'bproc', 'L', { label: 'Spawns' });
  link(d, 'ph', 'R', 'plug', 'L', { label: 'Starts' });
  hub(d, 'agent', 'L', 'bg', 'R', { label: 'Starts' });
  hub(d, 'agent', 'L', 'views', 'R', { label: 'Owns' });
  d.legend([{ kind: 'accent', label: 'Agent module' }, { kind: 'plugin', label: 'Plugin' }, { kind: 'process', label: 'Process · OS' }, { kind: 'line', label: 'Calls' }]);
  save('c4-3-components-agent-support', d, Y + GH);
}

// ---------- core ----------
{
  const d = P({ title: 'dyno-core — components', kind: 'C4 · Level 3 — Components of the Rust core', w: 1400,
    desc: 'Everything that decides is a pure function over values; time arrives as a parameter. The agent feeds it measurements and shows what it finds; dyno-platform implements its ports.' });
  const NH = 112, CW = 264, G = 96, x = 72, c = (i) => x + i * (CW + G);
  const gx = 48, gw = 24 + 4 * CW + 3 * G + 24, ay = Y, ah = 80, gy = ay + ah + 56;
  const r = (i) => gy + 40 + i * (NH + 32);
  const GH = 40 + NH * 3 + 32 * 3 + 112 + 24;
  d.node('agent', gx + gw / 2 - 160, ay, 320, ah, 'accent', { name: 'dyno-agent', tech: 'Rust, process', desc: 'Feeds values in, serves the results.' });
  d.group('g', gx, gy, gw, GH, 'dyno-core [Rust crate, pure]');
  const m = (id, col, row, name, tech, desc) => d.node(id, c(col), r(row), CW, NH, 'core', { name, tech, desc });
  m('model', 0, 0, 'metrics · process', 'modules', 'SystemSample, ProcessSample, ProcessRef, Reading<T>.');
  m('group', 1, 0, 'grouping', 'module', 'Processes into applications (ADR 0003).');
  m('diag', 2, 0, 'diagnosis · issues · baseline', 'modules', 'Why slow, disk trend, OS problem events, the baseline of programs and "Is this normal?" (ADR 0017, 0018).');
  m('health', 3, 0, 'health · lifespan · throttle', 'modules', 'Score with reasons only; throttling episodes, disk and battery life forecasts (ADR 0018).');
  m('hist', 0, 1, 'history · sampling · stats', 'modules', 'Tiers, min/avg/max downsampling, adaptive intervals (ADR 0002, 0008).');
  m('alerts', 1, 1, 'alerts · alarms · recorder', 'modules', 'Alert conditions, the alarm state machine, the flight-recorder ring (ADR 0013).');
  m('settings', 2, 1, 'settings · tray · counters', 'modules', 'Versioned settings, tray icon model, formatting, the performance counter model.');
  m('proto', 3, 1, 'protocol', 'module', 'Request, Reply, Message, LiveUpdate, PROTOCOL_VERSION.');
  m('helper', 0, 2, 'startup · power · wake', 'modules', 'Startup entries; keep awake, idle and timed actions, scheduled wakes, Wake-on-LAN (ADR 0032).');
  m('extras', 1, 2, 'extras', 'modules', 'What changed, sleep, boot, apps, GPU and files (ADR 0020).');
  m('web', 2, 2, 'weblinks · netchange · network', 'modules', 'Redacted search text and hash links; Wi-Fi, VPN and IP changes (ADR 0019).');
  m('snap', 3, 2, 'snapshot · report · inventory', 'modules', 'Snapshots and diffs, the weekly report, hardware and software inventory.');
  d.node('ports', c(0), r(3), 4 * CW + 3 * G, NH, 'core', { name: 'ports', tech: 'traits', desc: 'SystemSensors · ProcessSource · FileLocks · StartupItems · CounterSource · PowerActions · IdleSignals, among 16, plus WakeScheduler and WolReader. Contract: no panics, Unavailable with a reason, bounded calls, a start-time check on every ProcessRef. PowerActions (sleep, hibernate, shut down) and WakeScheduler (scheduled wakes) ask the OS to act, as the owner set them up.' });
  d.node('platform', gx + gw / 2 - 160, gy + GH + 56, 320, ah, 'accent', { name: 'dyno-platform', tech: 'Rust, adapters', desc: 'Implements the ports per OS.' });
  link(d, 'agent', 'B', 'g', 'T', { label: 'Calls' });
  link(d, 'platform', 'T', 'ports', 'B', { label: 'Implements', dash: true, arrow: 'open', t: [0.5, 0.5] });
  link(d, 'model', 'R', 'group', 'L', { label: 'Samples' });
  link(d, 'group', 'R', 'diag', 'L', { label: 'Apps' });
  d.legend([{ kind: 'core', label: 'Core module (pure)' }, { kind: 'accent', label: 'Agent · adapters' }, { kind: 'line', label: 'Uses' }, { kind: 'dashline', label: 'Implements' }]);
  save('c4-3-components-core', d, gy + GH + 56 + ah);
}
