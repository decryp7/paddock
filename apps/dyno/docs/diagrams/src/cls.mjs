import { P, save, link, cls } from './common.mjs';
const Y = 144;
const comp = { start: 'diamondThin', arrow: 'none' };
const LEG = [{ kind: 'accent', label: 'Type in dyno-core' }, { kind: 'core', label: 'Enumeration' }, { kind: 'line', label: 'Association (diamond: composition)' }, { kind: 'dashline', label: 'Dependency' }];

// ---------- measurements ----------
{
  const d = P({ title: 'Domain model — measurements', kind: 'UML · Class diagram', w: 1344,
    desc: 'Plain values with serde derives. Reading<T> makes "no sensor" explicit; a ProcessRef (pid plus start time) is the only way to name a process.' });
  const W = 320, G = 120, cx = (i) => 48 + i * (W + G);
  cls(d, 'sys', cx(0), Y, W, { name: 'SystemSample', attrs: ['at: Timestamp', 'cpu: Reading<CpuSample>', 'memory: Reading<MemorySample>', 'disks, disk_io, network', 'gpu, battery, power', 'temperatures, fans'] });
  cls(d, 'reading', cx(1), Y, W, { name: 'Reading<T>', stereotype: 'enumeration', role: 'core', attrs: ['Available(T)', 'Unavailable(reason: String)'] });
  const r1 = Y + d.rect.sys[3] + 72;
  cls(d, 'group', cx(0), r1, W, { name: 'AppGroup', attrs: ['id, name, exe', 'pids: Vec<Pid>', 'cpu, memory, disk_bps'] });
  cls(d, 'proc', cx(1), r1, W, { name: 'ProcessSample', attrs: ['pid, parent: Option<Pid>', 'name, exe, user', 'cpu, memory', 'disk_read_bps, write_bps', 'started: Timestamp', 'state: RunState'] });
  cls(d, 'pref', cx(2), r1, W, { name: 'ProcessRef', attrs: ['pid: Pid', 'started: Timestamp'] });
  link(d, 'sys', 'R', 'reading', 'L', { arrow: 'open', label: 'wraps' });
  link(d, 'group', 'R', 'proc', 'L', { ...comp, label: '1 to 1..*' });
  link(d, 'proc', 'R', 'pref', 'L', { dash: true, arrow: 'open', label: 'named by' });
  d.legend(LEG);
  save('uml-class-domain', d, r1 + d.rect.proc[3]);
}

// ---------- findings ----------
{
  const d = P({ title: 'Domain model — findings', kind: 'UML · Class diagram', w: 1208,
    desc: 'What Dyno tells the owner: causes, health reasons, alarms, OS issues, startup entries and history. Nothing here changes the machine.' });
  const W = 312, G = 88, cx = (i) => 48 + i * (W + G);
  cls(d, 'diag', cx(0), Y, W, { name: 'Diagnosis', attrs: ['headline: String', 'causes: Vec<Cause>'] });
  cls(d, 'cause', cx(1), Y, W, { name: 'Cause', attrs: ['kind, title, explanation', 'confidence: f32', 'evidence: Vec<Evidence>', 'hints: Vec<Hint> (text only)'] });
  cls(d, 'health', cx(2), Y, W, { name: 'HealthReport', attrs: ['score: u32, grade', 'reasons: Vec<HealthReason>', '(reasons only, no fixes)'] });
  const r1 = Y + d.rect.cause[3] + 72;
  cls(d, 'alarm', cx(0), r1, W, { name: 'Alarm', attrs: ['id, rule, key', 'severity: Severity', 'state: AlarmState', 'first_seen, last_seen', 'repeat_count, current_value'] });
  cls(d, 'astate', cx(1), r1, W, { name: 'AlarmState', stereotype: 'enumeration', role: 'core', attrs: ['Active', 'Acknowledged', 'Cleared', 'ClearedUnacknowledged'] });
  cls(d, 'issue', cx(2), r1, W, { name: 'Issue', attrs: ['id, at, kind: IssueKind', 'severity, app, title', 'evidence: String', 'source: IssueSourceId'] });
  const r2 = r1 + d.rect.alarm[3] + 72;
  cls(d, 'startup', cx(0), r2, W, { name: 'StartupEntry', attrs: ['id, name, command, exe', 'location: StartupLocation', 'state: StartupState (shown)', 'impact: StartupImpact'] });
  cls(d, 'hist', cx(1), r2, W, { name: 'HistoryPoint', attrs: ['at: Timestamp, tier', 'cpu, cpu_min, cpu_max', 'memory, disk, net (same shape)'] });
  link(d, 'diag', 'R', 'cause', 'L', { arrow: 'open', label: '0..*' });
  link(d, 'alarm', 'R', 'astate', 'L', { arrow: 'open', label: 'state', t: [(d.rect.astate[3] / 2) / d.rect.alarm[3], 0.5] });
  d.legend([LEG[0], LEG[1], { kind: 'line', label: 'Association' }]);
  save('uml-class-insight', d, r2 + Math.max(d.rect.startup[3], d.rect.hist[3]));
}
