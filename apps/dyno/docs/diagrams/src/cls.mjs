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

// ---------- rules and actions ----------
{
  const d = P({ title: 'Domain model — rules and actions', kind: 'UML · Class diagram', w: 1500,
    desc: 'A RuleSet holds rules; each rule is a ladder of steps that apply ProcessActions; every change is recorded as an ActivityEntry with the record that undoes it.' });
  const W = 336, G = 104, cx = (i) => 48 + i * (W + G);
  cls(d, 'ruleset', cx(0), Y, W, { name: 'RuleSet', attrs: ['rules: Vec<Rule>', 'protected: Vec<String>', 'profile: ProfileId', 'paused: bool'] });
  cls(d, 'rule', cx(1), Y, W, { name: 'Rule', attrs: ['id, name, enabled', 'condition: RuleCondition', 'ladder: Vec<RuleStep>', 'release_after_secs: u32', 'profiles: Vec<ProfileId>'] });
  cls(d, 'step', cx(2), Y, W, { name: 'RuleStep', attrs: ['action: StepAction', 'after_secs: u32'] });
  const r1 = Y + d.rect.rule[3] + 72;
  cls(d, 'engine', cx(0), r1, W, { name: 'Engine', attrs: ['state per (rule, app)'], ops: ['tick(Tick) -> Vec<Command>'] });
  cls(d, 'profile', cx(1), r1, W, { name: 'ProfileId', stereotype: 'enumeration', role: 'core', attrs: ['Normal · Gaming · Meeting', 'OnBattery · BuildingCode'] });
  cls(d, 'action', cx(2), r1, W, { name: 'ProcessAction', stereotype: 'enumeration', role: 'core', attrs: ['End', 'Suspend · Resume', 'SetPriority(Priority)', 'SetAffinity(mask)', 'SetEfficiencyMode(bool)', 'SetCpuCap(Option<u8>)'] });
  const r2 = Math.max(r1 + d.rect.action[3], r1 + d.rect.engine[3]) + 72;
  cls(d, 'activity', cx(0), r2, W, { name: 'ActivityEntry', attrs: ['id, at, kind', 'title, detail', 'undo: UndoState', 'bytes: Option<u64>'] });
  cls(d, 'undo', cx(2), r2, W, { name: 'UndoRecord', stereotype: 'enumeration', role: 'core', attrs: ['Process { target, actions }', 'Startup { entry_id, previous }', 'Tuning { id }', 'Quarantine { id }'] });
  link(d, 'ruleset', 'R', 'rule', 'L', { arrow: 'open', label: '0..*' });
  link(d, 'rule', 'R', 'step', 'L', { ...comp, label: '1..*' });
  link(d, 'step', 'B', 'action', 'T', { dash: true, arrow: 'open', label: 'applies' });
  link(d, 'engine', 'T', 'ruleset', 'B', { dash: true, arrow: 'open', label: 'evaluates' });
  link(d, 'rule', 'B', 'profile', 'T', { arrow: 'open', label: '0..*' });
  link(d, 'activity', 'R', 'undo', 'L', { arrow: 'open', label: 'holds', t: [0.5, (d.rect.activity[1] + d.rect.activity[3] / 2 - d.rect.undo[1]) / d.rect.undo[3]] });
  link(d, 'undo', 'T', 'action', 'B', { dash: true, arrow: 'open', label: 'undoes with' });
  d.legend(LEG);
  save('uml-class-rules', d, r2 + Math.max(d.rect.activity[3], d.rect.undo[3]));
}

// ---------- insight and changes ----------
{
  const d = P({ title: 'Domain model — diagnosis and changes', kind: 'UML · Class diagram', w: 1500,
    desc: 'What Dyno tells the owner (causes, health, alerts) and what it can change (startup entries, cleanup, admin tuning through the helper).' });
  const W = 336, G = 104, cx = (i) => 48 + i * (W + G);
  cls(d, 'diag', cx(0), Y, W, { name: 'Diagnosis', attrs: ['headline: String', 'causes: Vec<Cause>'] });
  cls(d, 'cause', cx(1), Y, W, { name: 'Cause', attrs: ['kind, title, explanation', 'confidence: f32', 'evidence: Vec<Evidence>', 'suggestions: Vec<Suggestion>'] });
  cls(d, 'health', cx(2), Y, W, { name: 'HealthReport', attrs: ['score: u32, grade', 'reasons: Vec<HealthReason>'] });
  const r1 = Y + d.rect.cause[3] + 72;
  cls(d, 'alert', cx(0), r1, W, { name: 'Alert', attrs: ['id, at, kind, severity', 'title, detail'] });
  cls(d, 'startup', cx(1), r1, W, { name: 'StartupEntry', attrs: ['id, name, command, exe', 'location: StartupLocation', 'state: StartupState', 'impact: StartupImpact'] });
  cls(d, 'hist', cx(2), r1 + 0, W, { name: 'HistoryPoint', attrs: ['at: Timestamp, tier', 'cpu, cpu_min, cpu_max', 'memory, disk, net (same shape)'] });
  const r2 = r1 + d.rect.startup[3] + 72;
  cls(d, 'scan', cx(0), r2, W, { name: 'CleanupScan', attrs: ['id, at, total_bytes', 'results: Vec<CategoryResult>', 'items: CleanupItem (path, bytes,', '  reason, selected)'] });
  cls(d, 'helper', cx(1), r2, W, { name: 'HelperAction', stereotype: 'enumeration', role: 'core', attrs: ['SetPowerScheme', 'SetVisualEffects', 'SetIndexingExcluded', 'SetDefenderExcluded', 'SetLowPowerMode', 'SetMachineStartupEnabled'] });
  cls(d, 'tuning', cx(2), r2, W, { name: 'TuningSuggestion', attrs: ['id, title, plan', 'preview lines, inverse'] });
  link(d, 'diag', 'R', 'cause', 'L', { arrow: 'open', label: '0..*' });
  link(d, 'helper', 'R', 'tuning', 'L', { dash: true, arrow: 'open', label: 'Used by', t: [(d.rect.tuning[3] / 2) / d.rect.helper[3], 0.5] });
  d.legend(LEG);
  save('uml-class-insight', d, r2 + d.rect.helper[3]);
}
