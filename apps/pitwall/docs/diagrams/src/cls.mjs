import { P, save, link, cls } from './common.mjs';
const Y = 144, W = 296, G = 104, VG = 64;
const comp = { start: 'diamondThin', arrow: 'none' };
const imp = { dash: true, arrow: 'open' };

/** items: [id, col, row, spec]; links: [a, sa, b, sb, label, opts] */
function classes(file, { title, desc, cols, items, links, legend }) {
  const d = P({ title, kind: 'UML · Class diagram', desc, w: 96 + cols * W + (cols - 1) * G });
  const rowY = {};
  const rows = Math.max(...items.map((i) => i[2])) + 1;
  const hs = Array.from({ length: rows }, () => 0);
  // measure first with a scratch diagram
  const scratch = P({ title: '', desc: '', w: 800 });
  for (const [id, c, r, spec] of items) hs[r] = Math.max(hs[r], cls(scratch, id, 0, 0, W, spec));
  let y = Y;
  for (let r = 0; r < rows; r++) { rowY[r] = y; y += hs[r] + VG; }
  for (const [id, c, r, spec] of items) cls(d, id, 48 + c * (W + G), rowY[r], W, spec);
  for (const [a, sa, b, sb, label, o = {}] of links) link(d, a, sa, b, sb, { label, ...o });
  d.legend(legend ?? [{ kind: 'accent', label: 'Server type' }, { kind: 'core', label: 'Core type, enum or trait' }, { kind: 'line', label: 'Has (diamond: owns)' }, { kind: 'dashline', label: 'Uses or implements' }]);
  save(file, d, y - VG);
}

classes('uml-class-serving', {
  title: 'Domain model — serving', desc: 'What decides and holds the models in memory: the server’s Serving owns hot models and a scheduler; the core’s policy says what to release.', cols: 4,
  items: [
    ['al', 0, 0, { name: 'ModelAlias', attrs: ['name: String', 'model: ModelId', 'context: Option<u32>', 'temperature: Option<f32>', 'system_prompt: Option<String>', 'engine: Option<EngineKind>'], ops: ['resolve(name) → Option<Resolved>'], role: 'core' }],
    ['sv', 1, 0, { name: 'Serving', attrs: ['hot: Map<ModelId, HotModel>', 'sched: Scheduler', 'aliases: Vec<ModelAlias>'], ops: ['resolve(ModelRef) → Resolved', 'acquire(app, model, prio) → Permit', 'ensure(ModelId, slots) → Busy', 'tick() → Vec<Action>', 'unload(ModelId, Reason)'] }],
    ['sc', 2, 0, { name: 'Scheduler', attrs: ['caps: Caps', 'paused: bool'], ops: ['submit(JobSpec, now) → JobId', 'dispatch(now) → Vec<JobId>', 'cancel(JobId) → Cancelled'], role: 'core' }],
    ['jb', 3, 0, { name: 'Job', attrs: ['id: JobId', 'spec: JobSpec', 'state: Queued | Running', 'submitted_at, started_at', 'progress, tokens'], role: 'core' }],
    ['hm', 1, 1, { name: 'HotModel', attrs: ['id: ModelId', 'ram, vram, context: u64', 'slots: u8, slot_cost: u64', 'active, last_used (atomics)', 'vision: bool, cold_start_ms'], ops: ['as_hot(pinned, idle) → Hot'] }],
    ['po', 2, 1, { name: 'Policy', attrs: ['idle_unload, deep_idle: Millis', 'pressure_available_percent: u8', 'max_hot: usize', 'yield_to_foreground: bool'], ops: ['plan_release(…) → Vec<Action>', 'plan_load(…) → LoadPlan'], role: 'core' }],
    ['pr', 3, 1, { name: 'Priority', stereotype: 'enum', attrs: ['Interactive', 'Background', 'Batch'], role: 'core' }],
    ['ps', 2, 2, { name: 'Preset', stereotype: 'enum', attrs: ['LowMemory', 'Balanced', 'Performance', 'Custom'], role: 'core' }],
  ],
  links: [['al', 'R', 'sv', 'L', 'resolves', imp], ['sv', 'R', 'sc', 'L', '1', comp], ['sc', 'R', 'jb', 'L', '0..*', comp], ['jb', 'B', 'pr', 'T', 'priority', { arrow: 'open' }], ['sv', 'B', 'hm', 'T', '0..*', comp], ['hm', 'R', 'po', 'L', 'plan_release', imp], ['po', 'B', 'ps', 'T', 'from', { arrow: 'open' }]],
});

classes('uml-class-library', {
  title: 'Domain model — library and engines', desc: 'Model files, the notices about newer ones, and the engines that run them: one trait, three implementations, one process handle.', cols: 3,
  items: [
    ['lb', 0, 0, { name: 'Library', ops: ['local() → Vec<LocalFile>', 'search(query)', 'download(repo, variant)', 'check_updates() → Vec<UpdateNotice>', 'delete(ModelId)'] }],
    ['lf', 1, 0, { name: 'LocalFile', attrs: ['id: ModelId', 'path, size', 'revision: String', 'info: GgufInfo', 'projector: Option<PathBuf>'], role: 'core' }],
    ['un', 2, 0, { name: 'UpdateNotice', attrs: ['model: ModelId', 'installed: Revision', 'available: Revision', 'better_quant: Option<Quant>', 'dismissed: bool'], role: 'core' }],
    ['ll', 0, 1, { name: 'LlamaCpp', attrs: ['pinned: Option<BuildId>', 'builds: Vec<Build>'], ops: ['install(build) with self-test', 'pin(BuildId), rollback()', 'capabilities() → BuildCaps', 'start(&Launch) → Running'] }],
    ['en', 1, 1, { name: 'Engine', stereotype: 'trait', attrs: [], ops: ['kind() → EngineKind', 'availability() → Availability', 'start(&Launch) → Running'], role: 'core' }],
    ['pk', 2, 1, { name: 'PackEngine', attrs: ['pack: EnginePack'], ops: ['install(), remove()', 'command_line(Launch)'] }],
    ['rn', 0, 2, { name: 'Running', attrs: ['child: Option<Child>', 'client: loopback::Client', 'port: u16', 'props: llama::Props'] }],
    ['pl', 1, 2, { name: 'Planned', attrs: ['kind: Mlx | Vllm | WhisperCpp', 'reason: String'], ops: ['availability() = NotAvailable'] }],
  ],
  links: [['lb', 'R', 'lf', 'L', '0..*', comp], ['lf', 'R', 'un', 'L', 'checked for', imp], ['ll', 'R', 'en', 'L', 'implements', imp], ['pk', 'L', 'en', 'R', 'implements', imp], ['pl', 'T', 'en', 'B', 'implements', imp], ['ll', 'B', 'rn', 'T', 'starts', { arrow: 'open' }]],
});

classes('uml-class-access', {
  title: 'Domain model — apps, keys and usage', desc: 'Who may call Pitwall and what it has used: apps own keys and permissions; usage history is kept per app with bounded retention.', cols: 3,
  items: [
    ['au', 0, 0, { name: 'Auth', attrs: ['apps: Vec<AppRecord> (max 64)', 'pending: pairing requests (max 8)', 'admin: String (admin.key)'], ops: ['authenticate(credential) → Principal', 'request_pairing, decide_pairing'] }],
    ['ar', 1, 0, { name: 'AppRecord', attrs: ['id: AppId, name', 'suite: bool', 'keys: Vec<StoredKey>', 'permissions: AppPermissions', 'last_seen_at'], role: 'core' }],
    ['sk', 2, 0, { name: 'StoredKey', attrs: ['id, shown (pw_3f9a1c…)', 'sha256: hex of the secret', 'created_at, last_used_at'], ops: ['matches(secret)  // constant-time'], role: 'core' }],
    ['uh', 0, 1, { name: 'UsageHistory', attrs: ['app: AppId', 'points: Vec<UsagePoint>', 'retention: Duration'], ops: ['record(now, tokens, requests, mem)', 'series(app, range) → Vec<UsagePoint>'], role: 'core' }],
    ['ap', 1, 1, { name: 'AppPermissions', attrs: ['models: Option<Vec<ModelId>>', 'max_priority: Priority', 'tools, extended_apis: bool', 'limits: AppLimits'], ops: ['authorize(…) → Priority'], role: 'core' }],
    ['lm', 2, 1, { name: 'AppLimits', attrs: ['requests_per_min: Option<u32>', 'tokens_per_day: Option<u64>', 'max_concurrent: Option<u32>', 'memory_cap: Option<u64>'], role: 'core' }],
    ['up', 0, 2, { name: 'UsagePoint', attrs: ['at: Timestamp', 'tokens: u64', 'requests: u32', 'peak_memory: u64'], role: 'core' }],
  ],
  links: [['au', 'R', 'ar', 'L', '0..64', comp], ['ar', 'R', 'sk', 'L', '0..*', comp], ['ar', 'B', 'ap', 'T', '1', comp], ['ap', 'R', 'lm', 'L', '1', comp], ['au', 'B', 'uh', 'T', 'records', { arrow: 'open' }], ['uh', 'B', 'up', 'T', '0..*', comp]],
});
