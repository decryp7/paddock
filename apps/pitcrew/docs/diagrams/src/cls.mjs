import { P, save, link, cls } from './common.mjs';
const Y = 144;
const LEG = [{ kind: 'accent', label: 'Type in pitcrew-core' }, { kind: 'core', label: 'Enumeration' }, { kind: 'line', label: 'Association (diamond: composition)' }, { kind: 'dashline', label: 'Dependency' }];
const comp = { start: 'diamondThin', arrow: 'none' };

// ---------- configuration ----------
{
  const d = P({ title: 'Domain model — configuration', kind: 'UML · Class diagram', w: 1344, desc: 'pitcrew.toml as types: defaults for every runner, one entry per runner, and the policies a runner can override.' });
  cls(d, 'cfg', 48, Y, 296, { name: 'Config', attrs: ['schema_version: u32', 'defaults: Defaults', 'runners: Vec<RunnerConfig>'], ops: ['load(primary, backup): Loaded', 'to_toml(&Config): String'] });
  cls(d, 'def', 48, Y + d.rect.cfg[3] + 56, 296, { name: 'Defaults', attrs: ['update: UpdatePolicy', 'update_timeouts: UpdateTimeouts', 'watchdog: StallRule', 'restart: RestartPolicy', 'logs: RotationPolicy', 'cleanup: CleanupPolicy'] });
  const rx = 48 + 296 + 112;
  cls(d, 'rc', rx, Y, 360, { name: 'RunnerConfig', attrs: ['name: RunnerName', 'kind: RunnerKind', 'dir: PathBuf', 'labels: Vec<String>', 'session: SessionMode', 'logged_off: LoggedOffMode', 'server_url: Option<String>', 'enabled: bool', 'binary_path: Option<PathBuf>', 'update: Option<UpdatePolicy>', 'watchdog: Option<StallRule>', 'cleanup: Option<CleanupPolicy>', 'env: Map<String, String>'] });
  const cx = rx + 360 + 112;
  cls(d, 'up', cx, Y, 296, { name: 'UpdatePolicy', attrs: ['mode: UpdateMode', 'pinned: Option<RunnerVersion>', 'window: Option<UpdateWindow>', 'check_every_hours: u32', 'channel: Channel', 'skipped: Vec<RunnerVersion>'] });
  cls(d, 'sr', cx, Y + d.rect.up[3] + 40, 296, { name: 'StallRule', attrs: ['threshold: u32', 'window_secs: u64'] });
  cls(d, 'cp', cx, d.rect.sr[1] + d.rect.sr[3] + 40, 296, { name: 'CleanupPolicy', attrs: ['automatic: bool', 'every_hours: u32', 'work_max_age_hours: u32', 'cache_cap_mb: u64', 'folder_cap_mb: u64', 'min_free_mb: u64', 'keep_previous: bool'] });
  link(d, 'cfg', 'B', 'def', 'T', { ...comp, label: '1' });
  link(d, 'cfg', 'R', 'rc', 'L', { ...comp, label: '0..16' });
  const sy = d.rect.rc[1] + 24, tx = rx + 360 + 56;
  for (const id of ['up', 'sr', 'cp']) {
    const cy = Math.round(d.rect[id][1] + 24);
    d.edge('rc', id, [rx + 360, Math.round(sy)], [cx, cy], { arrow: 'open', via: [[tx, Math.round(sy)], [tx, cy]] });
  }
  d.lbl('0..1 override', tx + 6, (d.rect.up[1] + d.rect.up[3] + d.rect.sr[1]) / 2, 'r');
  d.legend(LEG.slice(0, 3));
  save('uml-class-config', d, Math.max(d.rect.cp[1] + d.rect.cp[3], d.rect.def[1] + d.rect.def[3]));
}
// ---------- runtime ----------
{
  const d = P({ title: 'Domain model — runtime', kind: 'UML · Class diagram', w: 1344, desc: 'What the core decides with and what it tells the app: the controller, the update machine and the snapshot of a runner.' });
  cls(d, 'um', 48, Y, 320, { name: 'UpdateMachine', attrs: ['phase: UpdatePhase', 'timeouts: UpdateTimeouts'], ops: ['handle(UpdateEvent, now):', '  Result<Vec<UpdateCommand>>'] });
  cls(d, 'ct', 48, Y + d.rect.um[3] + 56, 320, { name: 'Controller', attrs: ['life: Life', 'pending: Option<Pending>', 'held: bool', 'job_running: bool', 'declared: bool', 'problem: Option<String>'], ops: ['handle(Input, now): Vec<Action>', 'facts(now, …): HealthFacts'] });
  cls(d, 'rs', 48 + 320 + 112, Y + 176, 232, { name: 'RunnerStatus', stereotype: 'enumeration', role: 'core', attrs: ['Stopped', 'Paused', 'WaitingForLogon', 'Draining', 'NotRegistered', 'CrashLoop', 'Updating', 'Busy', 'Stalled', 'Starting', 'Degraded', 'Idle'] });
  const sx = 48 + 320 + 112 + 232 + 112;
  cls(d, 'sn', sx, Y, 360, { name: 'RunnerSnapshot', attrs: ['name: RunnerName', 'kind: RunnerKind', 'status: RunnerStatus', 'pending: Option<Pending>', 'session: SessionState', 'running_since: Option<Timestamp>', 'next_start_at: Option<Timestamp>', 'restarts_24h: u32', 'problem: Option<String>', 'current_job: Option<CurrentJob>', 'current_version, latest_version', 'update_phase: UpdatePhase', 'last_update: Option<UpdateRecord>', 'disk: Option<DiskUse>', 'usage: Option<Usage>'] });
  cls(d, 'cj', sx, d.rect.sn[1] + d.rect.sn[3] + 56, 360, { name: 'CurrentJob', attrs: ['info: JobInfo', 'started_at: Timestamp'] });
  link(d, 'um', 'B', 'ct', 'T', { dash: true, arrow: 'open', label: 'Hold' });
  link(d, 'ct', 'R', 'rs', 'L', { dash: true, arrow: 'open', label: 'facts' });
  link(d, 'sn', 'L', 'rs', 'R', { arrow: 'open', label: '1' });
  link(d, 'sn', 'B', 'cj', 'T', { arrow: 'open', label: '0..1' });
  d.legend([{ kind: 'accent', label: 'Type in pitcrew-core' }, { kind: 'core', label: 'Enumeration' }, { kind: 'line', label: 'Association' }, { kind: 'dashline', label: 'Dependency' }]);
  save('uml-class-runtime', d, d.rect.cj[1] + d.rect.cj[3]);
}
// ---------- records ----------
{
  const d = P({ title: 'Domain model — records and metrics', kind: 'UML · Class diagram', w: 1504, desc: 'What the service keeps on disk: per-runner state and history, summaries for the app, and the tiered metric stores.' });
  const W = 272, G = 104, cx = (i) => 48 + i * (W + G);
  cls(d, 'ss', cx(0), Y, W, { name: 'ServiceState', attrs: ['runners: Map<String, RunnerState>', 'metrics: MetricsStore'] });
  cls(d, 'rst', cx(1), Y, W, { name: 'RunnerState', attrs: ['wanted: bool', 'paused: bool', 'history: History', 'last_update: Option<UpdateRecord>', 'latest: Option<RunnerVersion>', 'checked_at: Option<Timestamp>', 'last_cleanup: Option<CleanupReport>', 'folder_sizes: SizeSeries', 'usage: UsageStore', 'pid: Option<u32>'] });
  cls(d, 'hi', cx(2), Y, W, { name: 'History', attrs: ['events: ≤ 4000 HistoryEvent'], ops: ['push(at, kind)', 'recent(n)', 'restarts_since(t)'] });
  cls(d, 'he', cx(3), Y, W, { name: 'HistoryEvent', attrs: ['at: Timestamp', 'kind: HistoryKind'], ops: [] });
  const r1 = Y + Math.max(...['ss', 'rst', 'hi', 'he'].map((k) => d.rect[k][3])) + 56;
  cls(d, 'ts', cx(0), r1, 2 * W + G, { name: 'TieredStore<P>', attrs: ['raw: ring buffer', 'five_min: ring buffer', 'hourly: ring buffer'], ops: ['push(P)', 'query(now, range): Vec<P>'] });
  cls(d, 'hs', cx(3), r1, W, { name: 'HistorySummary', attrs: ['from, to: Timestamp', 'days: Vec<DaySummary>', 'uptime_permille: u32', 'restarts, jobs: u32', 'up, busy: Vec<Span>', 'marks: Vec<Mark>', 'updates: Vec<UpdateEntry>', 'recent_jobs: Vec<FinishedJob>'] });
  const r2 = r1 + d.rect.ts[3] + 56;
  cls(d, 'up', cx(0), r2, W, { name: 'UsagePoint', attrs: ['at: Timestamp', 'usage: Usage'] });
  cls(d, 'ms', cx(1), r2, W, { name: 'MachineSample', attrs: ['at: Timestamp', 'cpu_permille: u32', 'mem_used_bytes, mem_total_bytes', 'disk_free_bytes, disk_total_bytes', 'runners: Usage'] });
  cls(d, 'us', cx(2), r2, W, { name: 'Usage', attrs: ['cpu_permille, cpu_peak_permille', 'mem_bytes, mem_peak_bytes', 'processes: u32'] });
  link(d, 'ss', 'R', 'rst', 'L', { ...comp, label: '0..*' });
  link(d, 'rst', 'R', 'hi', 'L', { ...comp, label: '1' });
  link(d, 'hi', 'R', 'he', 'L', { ...comp, label: '0..4000' });
  link(d, 'he', 'B', 'hs', 'T', { dash: true, arrow: 'open', label: 'summarize' });
  d.edge('ss', 'ts', d.port('ss', 'B', 0.5), [cx(0) + W / 2, r1], {}); d.lbl('metrics', cx(0) + W / 2, (d.rect.ss[1] + d.rect.ss[3] + r1) / 2, 'r');
  d.edge('rst', 'ts', d.port('rst', 'B', 0.5), [cx(1) + W / 2, r1], { dash: false }); d.lbl('usage', cx(1) + W / 2, (d.rect.rst[1] + d.rect.rst[3] + r1) / 2, 'r');
  d.edge('ts', 'up', [cx(0) + W / 2, r1 + d.rect.ts[3]], d.port('up', 'T', 0.5), { arrow: 'open' }); d.lbl('P = UsagePoint', cx(0) + W / 2, r1 + d.rect.ts[3] + 28, 'r');
  d.edge('ts', 'ms', [cx(1) + W / 2, r1 + d.rect.ts[3]], d.port('ms', 'T', 0.5), { arrow: 'open' }); d.lbl('P = MachineSample', cx(1) + W / 2, r1 + d.rect.ts[3] + 28, 'r');
  link(d, 'ms', 'R', 'us', 'L', { arrow: 'open', label: 'runners' });
  d.legend(LEG);
  save('uml-class-records', d, r2 + Math.max(d.rect.up[3], d.rect.ms[3], d.rect.us[3]));
}
