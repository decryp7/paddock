import { P, save, link, hub } from './common.mjs';
const Y = 144;

// ---------- agent: requests and sampling ----------
{
  const d = P({ title: 'dyno-agent — requests and sampling', kind: 'C4 · Level 3 — Components of the agent', w: 1344,
    desc: 'A sampling loop feeds history, alarms and the flight recorder; an ipc server with one thread per connection answers the app; one SQLite writer holds the data.' });
  const NH = 112, CW = 248, G = 120, ax = 48, gx = ax + 216 + 72, c0 = gx + 24, c1 = c0 + CW + G, c2 = c1 + CW + G;
  const r = (i) => Y + 40 + i * (NH + 32);
  const GH = 40 + NH * 4 + 32 * 3 + 24;
  d.node('app', ax, r(0), 216, NH, 'accent', { name: 'App', tech: 'Tauri shell, client', desc: 'Subscribes and sends requests.' });
  d.group('g', gx, Y, 24 + 3 * CW + 2 * G + 24, GH, 'dyno-agent [Rust crate]', { os: true });
  d.node('server', c0, r(0), CW, NH, 'accent', { name: 'server', tech: 'ipc host, thread per connection', desc: 'Binds the private endpoint; checks token and protocol; bounded push queue.' });
  d.node('handlers', c1, r(0), CW, NH, 'accent', { name: 'handlers', tech: 'module', desc: 'One function per Request: protection check, validation, activity with undo.' });
  d.node('sampler', c0, r(1), CW, NH * 3 + 64, 'accent', { name: 'sampler', tech: 'thread', desc: 'Ticks at adaptive intervals: sensors, processes, groups, rules, diagnosis; pushes LiveUpdate; measures its own overhead.' });
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
    desc: 'What keeps the agent running and registered, the plugin host, and the benchmark runner that starts dyno-bench on a click. Every decision is a dyno-core call; every OS access a dyno-platform adapter.' });
  const NH = 112, CW = 216, G = 104, ox = 48, gx = ox + 176 + 72, c0 = gx + 24, c1 = c0 + CW + G, c2 = c1 + CW + G, ex = c2 + CW + 24 + 72;
  const r = (i) => Y + 40 + i * (NH + 32);
  const GH = 40 + NH * 6 + 32 * 5 + 24;
  d.node('sm', ox, r(1), 176, NH, 'process', { name: 'Service manager', tech: 'launchd · systemd · Run', desc: 'Starts the agent at login.' });
  d.group('g', gx, Y, 24 + 3 * CW + 2 * G + 24, GH, 'dyno-agent [Rust crate]', { os: true });
  d.node('cli', c0, r(0), CW, NH, 'accent', { name: 'cli', tech: 'module', desc: 'run · status · stop · once · bench · service install, uninstall, status, ensure.' });
  d.node('service', c0, r(1), CW, NH, 'accent', { name: 'service', tech: 'module', desc: 'Launch agent, systemd unit, Run entry. The unit text is pure.' });
  d.node('agent', c1, r(0), CW, NH * 6 + 160, 'accent', { name: 'agent (Shared)', tech: 'module', desc: 'Shared state behind locks that recover from poisoning; subscribers; wake and stop.' });
  d.node('disc', c2, r(0), CW, NH, 'accent', { name: 'discovery · files', tech: 'modules', desc: 'agent.json (owner only), data-folder layout, atomic JSON writes with backup.' });
  d.node('bench', c2, r(1), CW, NH, 'accent', { name: 'bench', tech: 'BenchCentre', desc: 'One kind at a time, a timeout each; removes its own scratch folder.' });
  d.node('ph', c2, r(2), CW, NH, 'accent', { name: 'dyno-plugin-host', tech: 'crate', desc: 'Starts plugins on first use; guards, quarantines.' });
  d.node('clean', c2, r(3), CW, NH, 'accent', { name: 'cleanup', tech: 'module', desc: 'Roots per OS, scanner, re-checking deleter, quarantine and restore.' });
  d.node('issues', c0, r(2), CW, NH + 16, 'accent', { name: 'issues · unusual', tech: 'two threads', desc: 'Read OS logs every 15 min; compare programs with the baseline every 30 s (ADR 0017).' });
  d.node('counters', c0, r(3), CW, NH, 'accent', { name: 'counters', tech: 'CounterCentre', desc: 'Counter browser under a deadline; pinned counters are read every 15 s.' });
  d.node('hwlife', c0, r(4), CW, NH, 'accent', { name: 'hardware · baselines', tech: 'modules', desc: 'Throttling episodes, disk and battery life, learned baselines (ADR 0018).' });
  d.node('hlink', c2, r(4), CW, NH, 'accent', { name: 'helper_link', tech: 'module', desc: 'Helper centre: install, status, uninstall and calls to the helper over dyno-ipc.' });
  d.node('extras', c0, r(5), CW, NH, 'accent', { name: 'extras · backup', tech: 'module', desc: 'Read-only OS tools, boot records and backup checks (ADR 0020).' });
  d.node('watch', c2, r(5), CW, NH, 'accent', { name: 'watch · netwatch', tech: 'modules', desc: 'Watched folder sweeps (ADR 0019); Wi-Fi, VPN and IP changes on a slow poll.' });
  d.node('hproc', ex, r(4), 216, NH, 'process', { name: 'dyno-helper', tech: 'system service', desc: 'Closed action list, audit log (ADR 0016).' });
  d.node('bproc', ex, r(1), 216, NH, 'process', { name: 'dyno-bench', tech: 'child process', desc: 'CPU, memory and disk tests.' });
  d.node('plug', ex, r(2), 216, NH, 'plugin', { name: 'Plugins', tech: 'out of process', desc: 'Vendor sensors, over dyno-ipc.' });
  hub(d, 'agent', 'L', 'cli', 'R', { rev: true, label: 'Runs' });
  link(d, 'cli', 'B', 'service', 'T', { label: 'Install' });
  link(d, 'service', 'L', 'sm', 'R', { label: 'Registers' });
  hub(d, 'agent', 'R', 'disc', 'L', { label: 'Writes' });
  hub(d, 'agent', 'R', 'bench', 'L', { label: 'Owns' });
  hub(d, 'agent', 'R', 'ph', 'L', { label: 'Owns' });
  hub(d, 'agent', 'R', 'clean', 'L', { label: 'Calls', dash: true });
  link(d, 'bench', 'R', 'bproc', 'L', { label: 'Spawns' });
  link(d, 'ph', 'R', 'plug', 'L', { label: 'Starts' });
  hub(d, 'agent', 'L', 'issues', 'R', { label: 'Starts' });
  hub(d, 'agent', 'R', 'hlink', 'L', { label: 'Owns' });
  hub(d, 'agent', 'L', 'counters', 'R', { label: 'Owns' });
  hub(d, 'agent', 'L', 'hwlife', 'R', { label: 'Owns' });
  hub(d, 'agent', 'L', 'extras', 'R', { label: 'Owns' });
  hub(d, 'agent', 'R', 'watch', 'L', { label: 'Owns' });
  link(d, 'hlink', 'R', 'hproc', 'L', { label: 'Requests' });
  d.legend([{ kind: 'accent', label: 'Agent module' }, { kind: 'plugin', label: 'Plugin' }, { kind: 'process', label: 'Process · OS' }, { kind: 'line', label: 'Calls' }]);
  save('c4-3-components-agent-support', d, Y + GH);
}

// ---------- core ----------
{
  const d = P({ title: 'dyno-core — components', kind: 'C4 · Level 3 — Components of the Rust core', w: 1400,
    desc: 'Everything that decides is a pure function over values; time arrives as a parameter. The agent feeds it measurements and carries out its decisions; dyno-platform implements its ports.' });
  const NH = 112, CW = 264, G = 96, x = 72, c = (i) => x + i * (CW + G);
  const gx = 48, gw = 24 + 4 * CW + 3 * G + 24, ay = Y, ah = 80, gy = ay + ah + 56;
  const r = (i) => gy + 40 + i * (NH + 32);
  const GH = 40 + NH * 3 + 32 * 3 + 112 + 24;
  d.node('agent', gx + gw / 2 - 160, ay, 320, ah, 'accent', { name: 'dyno-agent', tech: 'Rust, process', desc: 'Feeds values in, carries decisions out.' });
  d.group('g', gx, gy, gw, GH, 'dyno-core [Rust crate, pure]');
  const m = (id, col, row, name, tech, desc) => d.node(id, c(col), r(row), CW, NH, 'core', { name, tech, desc });
  m('model', 0, 0, 'metrics · process', 'modules', 'SystemSample, ProcessSample, ProcessRef, Reading<T>.');
  m('group', 1, 0, 'grouping', 'module', 'Processes into applications (ADR 0003).');
  m('rules', 2, 0, 'rules', 'module', 'Rule, Profile, RuleSet; the Engine state machine (ADR 0004).');
  m('protect', 3, 0, 'protect', 'module', 'The absolute protected list (ADR 0004).');
  m('hist', 0, 1, 'history · sampling · stats', 'modules', 'Tiers, min/avg/max downsampling, adaptive intervals (ADR 0002, 0008).');
  m('diag', 1, 1, 'diagnosis · issues · baseline', 'modules', 'Why slow, disk trend, OS problem events, the baseline of programs and "Is this normal?" (ADR 0017, 0018).');
  m('health', 2, 1, 'health · tuning · lifespan', 'modules', 'Score with reasons; tuning plans; throttling and disk and battery life forecasts (ADR 0006, 0018).');
  m('clean', 3, 1, 'cleanup', 'module', 'Catalogue and admission policy (ADR 0005).');
  m('act', 0, 2, 'activity · helper · startup', 'modules', 'Undo records, HelperAction validation, helper_wire, startup entries (ADR 0016).');
  m('alerts', 1, 2, 'alerts · alarms · recorder', 'modules', 'Alert rules, alarm state machine, flight-recorder ring.');
  m('settings', 2, 2, 'settings · tray · counters', 'modules', 'Versioned settings, tray icon model, formatting, the performance counter model.');
  m('proto', 3, 2, 'protocol', 'module', 'Request, Reply, Message, LiveUpdate, PROTOCOL_VERSION.');
  m('extras', 0, 3, 'extras · backup · toolpath', 'modules', 'Read-only views of the system, backup health and the login-shell PATH (ADR 0020).');
  m('watch', 1, 3, 'watch · weblinks · netchange', 'modules', 'Watched folders, redacted search text and hash links, Wi-Fi, VPN and IP changes (ADR 0019).');
  d.node('ports', c(2), r(3), 2 * CW + G, NH, 'core', { name: 'ports', tech: 'traits', desc: 'SystemSensors · ProcessSource · ProcessControl · FileLocks · StartupItems · FactsSource · UserActivity · CounterSource. Contract: no panics, Unavailable with a reason, bounded calls, a start-time check on every ProcessRef.' });
  d.node('platform', gx + gw / 2 - 160, gy + GH + 56, 320, ah, 'accent', { name: 'dyno-platform', tech: 'Rust, adapters', desc: 'Implements the ports per OS.' });
  link(d, 'agent', 'B', 'g', 'T', { label: 'Calls' });
  link(d, 'platform', 'T', 'ports', 'B', { label: 'Implements', dash: true, arrow: 'open', t: [0.5, 0.5] });
  link(d, 'rules', 'R', 'protect', 'L', { label: 'Consults' });
  link(d, 'rules', 'L', 'group', 'R', { label: 'Sees apps' });
  link(d, 'model', 'R', 'group', 'L', { label: 'Samples' });
  link(d, 'group', 'B', 'diag', 'T', { label: 'Apps' });
  d.legend([{ kind: 'core', label: 'Core module (pure)' }, { kind: 'accent', label: 'Agent · adapters' }, { kind: 'line', label: 'Uses' }, { kind: 'dashline', label: 'Implements' }]);
  save('c4-3-components-core', d, gy + GH + 56 + ah);
}
