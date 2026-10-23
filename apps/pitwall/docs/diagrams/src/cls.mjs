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
  title: 'Domain model — serving', desc: 'What decides and holds the models in memory: the server’s Serving resolves names (aliases first), owns hot models and a scheduler; the core’s policy says what to release and Slots says what a load costs.', cols: 4,
  items: [
    ['al', 0, 0, { name: 'ModelPreset', attrs: ['alias: AliasName', 'target: Target (ModelId | auto:…)', 'context: Option<u32>', 'temperature, top_p: Option<f32>', 'max_tokens: Option<u32>', 'system_prompt, engine: Option<String>'], ops: ['checked() → Result<ModelPreset, …>'], role: 'core' }],
    ['sv', 1, 0, { name: 'Serving', attrs: ['hot: Map<ModelId, HotModel>', 'sched: Scheduler', 'presets: Presets', 'gate: Option<CapabilityGate>'], ops: ['resolve(ModelRef, …) → Resolved', 'check_capabilities(Resolved, …)', 'acquire(app, model, …) → Permit', 'ensure_for(Resolved) → (Busy, ms)', 'slots_for(ModelId) → Slots', 'tick() → Vec<Action>', 'unload(ModelId, ReleaseReason)'] }],
    ['sc', 2, 0, { name: 'Scheduler', attrs: ['caps: Caps', 'slots: Map<ModelId, usize>', 'paused: bool'], ops: ['submit(JobSpec, now) → JobId', 'dispatch(now) → Vec<JobId>', 'set_model_slots(ModelId, usize)', 'cancel(JobId) → Cancelled'], role: 'core' }],
    ['jb', 3, 0, { name: 'Job', attrs: ['id: JobId', 'spec: JobSpec', 'state: Queued | Running', 'submitted_at, started_at', 'progress, tokens'], role: 'core' }],
    ['sl', 0, 1, { name: 'Slots', stereotype: 'newtype', attrs: ['u8, 1 to 16 (default 2)', 'context is per slot'], ops: ['new(n: u32) → Result<Slots, …>', 'launch_context(per_slot) → u32', '  // --ctx-size = context x slots'], role: 'core' }],
    ['hm', 1, 1, { name: 'HotModel', attrs: ['id: ModelId', 'ram, vram: u64', 'context: u32 (per slot)', 'slots: Slots', 'active, last_used (atomics)', 'vision: bool, cold_start_ms'], ops: ['as_hot(pinned, idle) → Hot'] }],
    ['po', 2, 1, { name: 'Policy', attrs: ['idle_unload, deep_idle: Millis', 'release_context_first: bool', 'pressure_available_percent: u8', 'min_free, max_hot, hot_cap', 'yield_to_foreground: bool'], ops: ['preset(Preset) → Policy', '  // memory::plan_release, plan_load'], role: 'core' }],
    ['pr', 3, 1, { name: 'Priority', stereotype: 'enum', attrs: ['Interactive', 'Background', 'Batch'], role: 'core' }],
    ['ps', 2, 2, { name: 'memory::Preset', stereotype: 'enum', attrs: ['LowMemory', 'Balanced', 'Performance', 'Custom'], role: 'core' }],
    ['cp', 0, 2, { name: 'cache (policy)', attrs: ['key: SHA-256 of fingerprint + request', 'ttl: 1 h … 90 d | never', 'max_bytes: 256 MiB | 512 MiB', 'affinity: conversation key → slot'], ops: ['eligible(req, app, preset) → bool', 'expired(entry, now) → bool', 'victims(entries, cap) → Vec<Key>'], role: 'core' }],
    ['cs', 1, 2, { name: 'CacheStore', attrs: ['memory: LRU map', 'disk: cache.sqlite (WAL)', 'writer: thread'], ops: ['get(key, now) → Option<Value>', 'put(key, value, meta)', 'sweep(now, 5000)', 'clear(scope), info()'] }],
  ],
  links: [['al', 'R', 'sv', 'L', 'resolves', imp], ['sv', 'R', 'sc', 'L', '1', comp], ['sc', 'R', 'jb', 'L', '0..*', comp], ['jb', 'B', 'pr', 'T', 'priority', { arrow: 'open' }], ['sv', 'B', 'hm', 'T', '0..*', comp], ['hm', 'L', 'sl', 'R', 'slots', { arrow: 'open' }], ['hm', 'R', 'po', 'L', 'judged by', imp], ['po', 'B', 'ps', 'T', 'from', { arrow: 'open' }], ['cs', 'L', 'cp', 'R', 'decided by', imp]],
});

classes('uml-class-library', {
  title: 'Domain model — library and engines', desc: 'Model files with their origin, the update check that only asks, and the engines that run them: one trait with one implementation (llama.cpp), pins and a capability matrix.', cols: 3,
  items: [
    ['lf', 0, 0, { name: 'LocalFile', attrs: ['id: ModelId', 'path, size', 'info: GgufInfo', 'projector: Option<PathBuf>', 'repo: String'], role: 'accent' }],
    ['lb', 1, 0, { name: 'Library', ops: ['local() → Vec<LocalFile>', 'installed() → Vec<Installed>', 'search(query)', 'repo(repo, context, room) → HfRepo', 'download(repo, variant) → id', 'delete(ModelId)'] }],
    ['pv', 2, 0, { name: 'Provenance', stereotype: 'sidecar', attrs: ['<file>.origin.json', 'version, repo, file, size', 'sha256: Vec<String>', 'revision: Option<String>', 'downloaded_at'], ops: ['parse(text) → Option<Provenance>'], role: 'core' }],
    ['rs', 0, 1, { name: 'RepoSource', stereotype: 'trait', attrs: [], ops: ['snapshot(RepoId) → RepoSnapshot'] }],
    ['up', 1, 1, { name: 'Updates', attrs: ['state: UpdateState', 'pace, timeout'], ops: ['check(source, Request) → Report', 'dismiss(id, snooze_days, now)', 'downloaded(repo, file)'] }],
    ['un', 2, 1, { name: 'UpdateNotice', attrs: ['id: <model>#<kind>', 'model_id, repo, file', 'kind: UpdateKind', 'size_bytes, quant, summary', 'installed_file, installed_quant', 'found_at, target_sha256'], ops: ['fingerprint() → String'], role: 'core' }],
    ['en', 1, 2, { name: 'Engine', stereotype: 'trait', attrs: [], ops: ['kind() → EngineKind', 'availability() → Availability', 'start(&Launch) → Running'], role: 'core' }],
    ['ep', 0, 3, { name: 'EnginePins', attrs: ['global: Option<BuildId>', 'models: Map<String, BuildId> (max 256)', 'history: Vec<BuildId> (max 8)'], ops: ['set_global, set_model(…, installed)', 'remember(BuildId)', 'pins::select(…) → Selection', 'pins::rollback_target(…) → BuildId'], role: 'core' }],
    ['ll', 1, 3, { name: 'LlamaCpp', attrs: ['pins: EnginePins', 'matrix: Matrix', 'choice: BuildChoice'], ops: ['install(id, …) → BuildInfo', 'select_for(model) → Selection', 'check(model, needed) → Result', 'start(&Launch) → Running'] }],
    ['rn', 2, 3, { name: 'Running', attrs: ['child: Option<Child>', 'client: loopback::Client', 'port: u16', 'props: llama::Props'] }],
    ['mx', 1, 4, { name: 'Matrix', attrs: ['entries: Capability → Requirement', 'min_build, unsupported_backends'], ops: ['gate(needed, BuildFacts) → Gate'], role: 'core' }],
  ],
  links: [['lb', 'L', 'lf', 'R', '0..*', comp], ['lb', 'R', 'pv', 'L', 'writes', { arrow: 'open' }], ['lb', 'B', 'up', 'T', 'installed()', { arrow: 'open' }], ['up', 'L', 'rs', 'R', 'asks', imp], ['up', 'R', 'un', 'L', 'raises', { arrow: 'open' }], ['ll', 'T', 'en', 'B', 'implements', imp], ['ll', 'L', 'ep', 'R', 'holds', comp], ['ll', 'R', 'rn', 'L', 'starts', { arrow: 'open' }], ['ll', 'B', 'mx', 'T', 'holds', comp]],
});

classes('uml-class-access', {
  title: 'Domain model — apps, keys and usage', desc: 'Who may call Pitwall and what it has used: apps own keys and permissions; AppState holds the usage store, which keeps hourly and daily buckets per app.', cols: 4,
  items: [
    ['as', 0, 0, { name: 'AppState', attrs: ['auth: Auth', 'usage: Usage', 'serving: Serving', 'updates: Updates'], ops: ['start_chat(Principal, …) → Run', 'save_usage()  // usage.json'] }],
    ['au', 1, 0, { name: 'Auth', attrs: ['apps: Vec<AppRecord> (max 64)', 'pending: pairing requests (max 8)', 'admin: String (admin.key)', 'rates: per-app window'], ops: ['authenticate(credential, …) → Principal', 'admit(app, per_min, now)', 'request_, decide_, poll_pairing'] }],
    ['ar', 2, 0, { name: 'AppRecord', attrs: ['id: AppId, name', 'suite: bool', 'keys: Vec<StoredKey>', 'permissions: AppPermissions', 'last_seen_at'], role: 'core' }],
    ['sk', 3, 0, { name: 'StoredKey', attrs: ['id, shown (pw_3f9a1c…)', 'sha256: hex of the secret', 'created_at, last_used_at'], ops: ['keys::matches(secret, key)'], role: 'core' }],
    ['uh', 0, 1, { name: 'Usage', attrs: ['hours: Buckets (kept 7 days)', 'days: Buckets (kept 90 days)', 'per bucket: Map<app, Counters>'], ops: ['record(&UsageEvent)', 'series(range, now, app?) → UsageSeries', 'budget_use(at, app) → BudgetUse', 'restore(json, now), to_json()'], role: 'core' }],
    ['us', 1, 1, { name: 'UsageSeries', attrs: ['range: 24h | 7d | 30d', 'step_ms, starts: Vec<Millis>', 'total, totals: Counters', 'apps: Vec<AppUsageSeries>'], role: 'core' }],
    ['ap', 2, 1, { name: 'AppPermissions', attrs: ['models: Option<Vec<ModelId>>', 'max_priority: Priority', 'tools, extended_apis: bool', 'limits: AppLimits'], ops: ['authorize(…) → Priority'], role: 'core' }],
    ['lm', 3, 1, { name: 'AppLimits', attrs: ['requests_per_min: Option<u32>', 'requests_per_day: Option<u64>', 'tokens_per_day, tokens_per_month', 'max_concurrent: Option<u32>', 'max_memory_percent: Option<u8>'], role: 'core' }],
    ['cn', 1, 2, { name: 'Counters', attrs: ['requests, errors: u64', 'prompt_tokens, completion_tokens', 'busy_ms: u64', 'peak_memory_bytes  // attributed'], role: 'core' }],
  ],
  links: [['as', 'R', 'au', 'L', 'owns', comp], ['au', 'R', 'ar', 'L', '0..64', comp], ['ar', 'R', 'sk', 'L', '0..*', comp], ['ar', 'B', 'ap', 'T', '1', comp], ['ap', 'R', 'lm', 'L', '1', comp], ['as', 'B', 'uh', 'T', 'owns', comp], ['uh', 'R', 'us', 'L', 'series()', { arrow: 'open' }], ['us', 'B', 'cn', 'T', '0..*', comp]],
});
