import { P, save, link } from './common.mjs';
const Y = 144, W = 264, GAP = 96, NH = 120, VG = 40;

/**
 * A component diagram on a grid. nodes: [id, col, row, name, tech, desc, { planned, role, span }]; links: [a, sa, b, sb, label, opts].
 */
function panel(file, { title, kind, desc, group, cols, rows, nodes, links, legend }) {
  const w = 96 + cols * W + (cols - 1) * GAP + 48;
  const d = P({ title, kind, desc, w: Math.max(w, 1000) });
  const gx = 48, gw = 24 + cols * W + (cols - 1) * GAP + 24, gh = 40 + rows * NH + (rows - 1) * VG + 24;
  d.group('g', gx, Y, gw, gh, group);
  for (const [id, c, r, name, tech, dsc, o = {}] of nodes) {
    const span = o.span ?? 1;
    d.node(id, 72 + c * (W + GAP), Y + 40 + r * (NH + VG), W, NH * span + VG * (span - 1), o.role ?? 'core', { name, tech, desc: dsc, planned: o.planned });
  }
  for (const [a, sa, b, sb, label, o = {}] of links) {
    const { rev, ...rest } = o;
    if (rev) link(d, b, sb, a, sa, { label, ...rest }); else link(d, a, sa, b, sb, { label, ...rest });
  }
  d.legend(legend ?? [{ kind: 'core', label: 'Component (pure Rust)' }, { kind: 'line', label: 'Feeds' }]);
  save(file, d, Y + gh);
}
const LEGP = [{ kind: 'core', label: 'Component (pure Rust)' }, { kind: 'line', label: 'Feeds' }];
const SUB = 'C4 · Level 3 — Components of pitwall-core';

panel('c4-3-core-wire', {
  title: 'pitwall-core — wire formats', kind: SUB, group: 'Wire formats and requests [pure, no I/O]', cols: 3, rows: 2,
  desc: 'Each API is parsed once into one neutral request and answered in its own family’s shape. Time and bytes come in as values (ADR 0002, 0003, 0019).',
  nodes: [
    ['api', 0, 0, 'api', 'openai · anthropic · ollama', 'Parse each API’s request once; write answers, stream chunks and errors in that family’s shape.'],
    ['chat', 1, 0, 'chat', 'neutral request', 'ChatRequest, ModelRef, ChatEvent, Usage: what every API maps to.'],
    ['llama', 2, 0, 'llama', 'llama-server protocol', 'Launch arguments, parallel slots, the /v1/chat/completions body and /props parsing.'],
    ['tasks', 1, 1, 'tasks · tokens', '/x/v1 helpers', 'Versioned task prompts (summarise, classify, extract, translate); token estimates and truncation.'],
    ['sse', 2, 1, 'sse', 'event streams', 'SSE and NDJSON framing for streamed answers.'],
    ['cache', 0, 1, 'cache', 'keys · expiry · eviction', 'ADR 0019: response and embeddings cache keys, expiry options, LRU eviction and sweep decisions; the store is in the server.'],
  ],
  links: [['api', 'R', 'chat', 'L', 'Parse into'], ['chat', 'R', 'llama', 'L', 'Maps to'], ['llama', 'B', 'sse', 'T', 'Events'], ['tasks', 'T', 'chat', 'B', 'Builds', { rev: true }], ['cache', 'T', 'api', 'B', 'Keyed by', { rev: true }]],
});

panel('c4-3-core-serving', {
  title: 'pitwall-core — serving decisions', kind: SUB, group: 'Serving [ADR 0005, 0006, 0014, 0016]', cols: 4, rows: 2,
  desc: 'Which model answers, whether it fits, how many requests run at once and what to give back. Every function takes the facts it needs as arguments.',
  nodes: [
    ['presets', 0, 0, 'presets', 'ModelPreset · AliasName', 'An alias maps to a model or an auto: selection plus context, sampling defaults, system prompt and engine. resolve and apply; presets.json.'],
    ['select', 1, 0, 'select', 'capability choice', 'Requirements::parse_auto and choose, with fit verdicts and measured speed; says how it chose.'],
    ['fit', 2, 0, 'fit', 'will it fit?', 'ModelSize, Room, Verdict (Fits, Tight, WontFit, Unknown); need_tokens and assess for a total number of KV positions.'],
    ['slots', 3, 0, 'slots', 'Slots 1 to 16', 'Context is per slot; launch_context = context x slots. solve builds the fit table; affinity picks the slot (id_slot) for a conversation.'],
    ['sched', 0, 1, 'scheduler', 'priority queue', 'Priorities, global, per-model (slots) and per-app caps, aging, cancel, pause, progress.'],
    ['mem', 1, 1, 'memory', 'plan_load · plan_release', 'Memory presets, idle and deep-idle, pressure, yield; never unloads a busy model.'],
    ['pres', 2, 1, 'pressure · supervise', 'OS signals · restarts', 'Turns raw OS signals into levels; restart delays and the crash-loop guard for engines (backoff).'],
  ],
  links: [['presets', 'R', 'select', 'L', 'Auto target'], ['fit', 'L', 'select', 'R', 'Verdicts', { }], ['slots', 'L', 'fit', 'R', 'Sizes with'], ['select', 'B', 'mem', 'T', 'Chosen model'], ['sched', 'R', 'mem', 'L', 'Active jobs'], ['pres', 'L', 'mem', 'R', 'Levels']],
});

panel('c4-3-core-models', {
  title: 'pitwall-core — models and engines', kind: SUB, group: 'Models and engines [ADR 0004, 0011, 0017, 0018, 0020]', cols: 4, rows: 3, legend: LEGP,
  desc: 'What the files and builds are, what is newer, which build serves and what it can do. Server code downloads; these modules only judge.',
  nodes: [
    ['hf', 1, 0, 'hf', 'Hugging Face', 'Parses search, tree and info answers; builds checked URLs (tree_url, info_url, file_url).'],
    ['upd', 0, 0, 'updates', 'update rules', 'Provenance sidecar, Installed::from_disk, evaluate into UpdateNotice (NewerRevision, BetterQuant, Imatrix). Notice only.'],
    ['gguf', 2, 0, 'gguf · quant', 'file headers', 'Reads GGUF headers with limits; quantisation names and sizes.'],
    ['fit', 3, 0, 'fit', 'will it fit?', 'ModelSize, Room, Verdict; slots.rs adds the per-slot cost.'],
    ['packs', 1, 1, 'packs', 'ADR 0011', 'Model catalog packs parsed once; every problem named.'],
    ['rt', 2, 1, 'runtimes', 'catalog · builds', 'Pinned llama.cpp catalog, BuildId, BuildChoice and the automatic order.'],
    ['hw', 3, 1, 'hardware', 'probes parsing', '--list-devices and nvidia-smi parsing; RAM and GPU facts.'],
    ['ust', 0, 1, 'update_state', 'model-updates.json', 'Schedule (weekly or daily), notices, per-offer dismissals and snoozes; bounded and versioned.'],
    ['pins', 2, 2, 'runtimes::pins', 'engine-pins.json', 'EnginePins: select (model pin, global pin, automatic), rollback_target, moved; strict PinError.'],
    ['caps', 3, 2, 'runtimes::capabilities', 'capabilities.json', 'Matrix::gate(needed, BuildFacts) gives Allowed, NeedsBuild or NotSupported.'],
  ],
  links: [['hf', 'L', 'upd', 'R', 'Listings'], ['gguf', 'R', 'fit', 'L', 'Sizes'], ['hw', 'T', 'fit', 'B', 'Room'], ['hw', 'L', 'rt', 'R', 'Devices'], ['rt', 'B', 'pins', 'T', 'Builds'], ['pins', 'R', 'caps', 'L', 'Chosen build'], ['upd', 'B', 'ust', 'T', 'Notices']],
});

panel('c4-3-core-state', {
  title: 'pitwall-core — security and state', kind: SUB, group: 'Security and state [ADR 0001, 0007, 0013]', cols: 3, rows: 3,
  desc: 'What may leave the machine, what a key may do, and the versioned records the server keeps. Reading damaged files falls back to defaults.',
  nodes: [
    ['priv', 0, 0, 'privacy', 'ADR 0001', 'DownloadUrl allow-list, Host and Origin rules.'],
    ['keys', 1, 0, 'keys · limits', 'ADR 0007, 0013', 'pw_ keys, SHA-256, constant-time match, pairing codes, permissions; AppLimits with check_budget, check_memory, statuses.'],
    ['usage', 2, 0, 'usage', 'usage.json', 'Counters per app in hourly (7 days) and daily (90 days) buckets; series for 24 h, 7 d and 30 d; budget_use.'],
    ['cfg', 0, 1, 'config', 'settings.toml', 'Versioned Settings: memory, default and per-model slots, update-check schedule; damage tolerance, backup.'],
    ['met', 1, 1, 'metrics · events', 'bounded series', 'Time series with retention, the events feed, per-app counters.'],
    ['bench', 1, 2, 'benchmarks · redact', 'results · content redaction', 'Stored benchmark results that feed fast selection; redaction of request content before it is kept.'],
    ['mgmt', 2, 1, 'mgmt · discovery', 'wire contract', 'DTOs the UI, the shell and the CLI share (api.types.ts); discovery.json.'],
  ],
  links: [['keys', 'R', 'usage', 'L', 'Budgets'], ['usage', 'B', 'mgmt', 'T', 'UsageSeries'], ['cfg', 'R', 'met', 'L', 'Retention'], ['mgmt', 'L', 'met', 'R', 'Shapes', { rev: true }], ['met', 'B', 'bench', 'T', 'Speeds']],
});

const SUBS = 'C4 · Level 3 — Components of pitwall-server';
panel('c4-3-server-requests', {
  title: 'pitwall-server — request path', kind: SUBS, group: 'Request path [ADR 0003, 0007, 0018, 0019]', cols: 3, rows: 3,
  desc: 'Every request passes the gate, then one pipeline. Decisions come from pitwall-core; this crate sockets, files and processes.',
  nodes: [
    ['http', 0, 0, 'http', 'axum router', 'The three standard APIs and /x/v1; the gate checks Host, Origin and key; provenance headers incl. X-Pitwall-Alias; SSE and NDJSON out.', { role: 'accent' }],
    ['mgmt', 0, 1, 'mgmt', '/mgmt/v1', 'One JSON call per UI method (rpc/{method}), the live feed and the playground stream; admin key only.', { role: 'accent' }],
    ['app', 1, 0, 'app', 'AppState', 'start_chat: resolve alias, apply preset, authorize, limits, capability gate, queue, load, generate; the Recorder; usage.json, presets.json and engine-pins.json saves.', { role: 'accent', span: 2 }],
    ['auth', 2, 0, 'auth', 'apps.json', 'Apps, hashed keys, pairing, admin key file, per-app rate window.', { role: 'accent' }],
    ['serving', 2, 1, 'serving', 'resolve, queue, load, release', 'Resolve names, acquire a turn, ensure_for a hot model with its slots and slot affinity, idle release every 5 s, chat and embed on the engine.', { role: 'accent' }],
    ['cache', 1, 2, 'cache', 'responses · embeddings', 'Lookup before the queue (a hit needs no model), store after a completed answer; sweep at start, hourly and when idle; X-Pitwall-Cache.', { role: 'accent' }],
    ['cdb', 2, 2, 'cache.sqlite', 'memory or one SQLite file', 'WAL, versioned schema, owner-only; written by a writer thread off the request path; LRU over the size cap.', { role: 'process' }],
  ],
  links: [['http', 'R', 'app', 'L', 'Requests', { t: [0.5, 0.2143] }], ['mgmt', 'R', 'app', 'L', 'Rpc calls', { t: [0.5, 0.7857] }], ['app', 'R', 'auth', 'L', 'Keys', { t: [0.2143, 0.5] }], ['app', 'R', 'serving', 'L', 'Starts', { t: [0.7857, 0.5] }], ['app', 'B', 'cache', 'T', 'Lookup, store'], ['cache', 'R', 'cdb', 'L', 'Writes']],
  legend: [{ kind: 'accent', label: 'Server module' }, { kind: 'process', label: 'Data' }, { kind: 'line', label: 'Calls' }],
});

panel('c4-3-server-edges', {
  title: 'pitwall-server — engines and outbound', kind: SUBS, group: 'Engines, library and the edges [ADR 0001, 0004, 0011, 0017, 0018, 0020]', cols: 4, rows: 3,
  desc: 'Outbound HTTP exists only in egress, and child processes are started only through process. The update check is opt-in and read-only.',
  nodes: [
    ['lib', 0, 0, 'library', 'models', 'Scans GGUF files, Hugging Face search, repo file lists with fit and max slots, resumable verified downloads that write <file>.origin.json, delete.', { role: 'accent' }],
    ['eg', 1, 0, 'egress', 'the only HTTP client', 'Allow-listed HTTPS, redirects re-checked, resume, SHA-256.', { role: 'accent' }],
    ['eng', 2, 0, 'engines', 'Engine trait · LlamaCpp', 'The only engine: install with self-test, remove, start on the pinned or automatic build; also the CapabilityGate.', { role: 'accent' }],
    ['pins', 3, 0, 'engine_pins', 'pin, rollback, capabilities', 'state, set_global, set_model, rollback, capabilities; saves engine-pins.json; reports appliesOnNextLoad.', { role: 'accent' }],
    ['sys', 0, 1, 'system · paths', 'probes and files', 'RAM, GPU, disk, engine memory; Dirs for the data folder, atomic writes with backup.', { role: 'accent' }],
    ['upd', 1, 1, 'updates', 'check only · opt-in', 'HfSource over egress; Updates::check; schedule from 60 s after start, then every 5 min when enabled; writes model-updates.json. Never downloads.', { role: 'accent' }],
    ['proc', 2, 1, 'process', 'only place that spawns', 'Scrubbed environment, no console window, killed on drop.', { role: 'accent' }],
    ['setup', 3, 1, 'engine_setup', 'ADR 0020 · automatic install', 'On start, and when a model needs a build and none is installed, installs the suggested one in the background; loads wait for it; Retry after an error.', { role: 'accent' }],
    ['svc', 3, 2, 'service', 'spec · systemd · windows', 'Per-OS names, unit text, Windows wrapper, restart after an update.', { role: 'accent' }],
  ],
  links: [['lib', 'R', 'eg', 'L', 'Downloads'], ['eng', 'L', 'eg', 'R', 'Installs', { }], ['eng', 'R', 'pins', 'L', 'Holds pins'], ['eng', 'B', 'proc', 'T', 'Spawns via'], ['lib', 'B', 'sys', 'T', 'Disk, RAM'], ['eg', 'B', 'upd', 'T', 'Snapshots via', { rev: true, dash: true }]],
  legend: [{ kind: 'accent', label: 'Server module' }, { kind: 'line', label: 'Calls' }, { kind: 'dashline', label: 'Check only, opt-in' }],
});

const SUBU = 'C4 · Level 3 — Components of the Angular UI';
const FP = 'features/';
panel('c4-3-ui-features', {
  title: 'Angular UI — screens', kind: SUBU, group: 'Features [standalone components, OnPush, one page each]', cols: 4, rows: 3,
  desc: 'Each screen binds to its own SignalStore and shows what the server computes; none re-implements a domain rule. The stores are in the next diagram.',
  legend: [{ kind: 'ui', label: 'Screen' }],
  nodes: [
    ['dash', 0, 0, 'dashboard', `${FP}dashboard`, 'Stat tiles, loaded models with idle countdown, queue, pairing card, events feed, usage card.', { role: 'ui' }],
    ['models', 1, 0, 'models', `${FP}models`, 'Library, Hugging Face discover with the fit check, presets tab, slots panel with the fit table, build menu per model.', { role: 'ui' }],
    ['engines', 2, 0, 'engines', `${FP}engines`, 'Builds, install with progress, self-test, version pin, roll back, features per build (supported, needs bNNNN)', { role: 'ui' }],
    ['apps', 3, 0, 'apps', `${FP}apps`, 'Apps and keys, pairing approval, permissions and limits, per-app usage over time, key shown once.', { role: 'ui' }],
    ['req', 0, 1, 'requests', `${FP}requests`, 'Recent requests with timing, the chosen model and alias; prompts only on demand.', { role: 'ui' }],
    ['play', 1, 1, 'playground', `${FP}playground`, 'Chat with a local model, streamed through the management API.', { role: 'ui' }],
    ['bench', 2, 1, 'benchmarks', `${FP}benchmarks`, 'Tokens per second and time to first token per model.', { role: 'ui' }],
    ['set', 3, 1, 'settings', `${FP}settings`, 'General, server, memory presets and idle release, network, privacy.', { role: 'ui' }],
    ['tray', 0, 2, 'tray', `${FP}tray`, 'Compact view for the tray and menu bar.', { role: 'ui' }],
    ['usage', 1, 2, 'usage', `${FP}usage`, 'Usage card (24 h, 7 d, 30 d) and the per-app view: tokens, requests, attributed peak memory, limit statuses.', { role: 'ui' }],
    ['upd', 2, 2, 'updates', `${FP}updates`, 'Update banner: Get it (confirm host and size), Dismiss, Remind me in a week; check status.', { role: 'ui' }],
  ],
  links: [],
});

panel('c4-3-ui-state', {
  title: 'Angular UI — state and backend', kind: SUBU, group: 'State and the backend boundary', cols: 4, rows: 2,
  desc: 'Components never call Tauri; they use the backend injected by the BACKEND token. The real backend invokes the shell; the mock serves browser development and tests only.',
  legend: [{ kind: 'ui', label: 'Angular UI' }, { kind: 'accent', label: 'Pitwall container' }, { kind: 'line', label: 'Calls' }],
  nodes: [
    ['stores', 0, 0, 'Feature stores', 'one SignalStore per page', 'Load through the backend; keep loading, error and Retry state; expose signals.', { role: 'ui' }],
    ['live', 0, 1, 'LiveStore', 'core/live.store · live-reducer', 'Applies pushed updates (memory, models, queue, throughput, pairing) so tiles move without polling.', { role: 'ui' }],
    ['shared', 3, 1, 'Shared UI', 'tokens · charts · icons', 'Design tokens (dark, light), ECharts wrapper, own SVG icons, native-feel rules, toasts.', { role: 'ui' }],
    ['be', 1, 0, 'PitwallBackend', 'interface + BACKEND token', 'One method per management call and a live feed; api.types.ts is the wire contract that pitwall_core::mgmt mirrors.', { role: 'ui', span: 2 }],
    ['tauri', 2, 0, 'TauriBackend', 'core/tauri-backend', 'Invokes the shell’s commands and subscribes to its live event. The only code that calls Tauri.', { role: 'ui' }],
    ['mock', 2, 1, 'MockBackend', 'browser development and tests only', 'Plausible data; never the reference for behaviour.', { role: 'ui' }],
    ['shell', 3, 0, 'Rust shell', 'app/src-tauri · Tauri IPC', 'Commands to the server’s management API.', { role: 'accent' }],
  ],
  links: [['stores', 'R', 'be', 'L', 'Call', { t: [0.5, 0.2143] }], ['live', 'R', 'be', 'L', 'Subscribes', { t: [0.5, 0.7857] }], ['tauri', 'L', 'be', 'R', 'Implements', { t: [0.5, 0.2143] }], ['mock', 'L', 'be', 'R', 'Implements', { t: [0.5, 0.7857] }], ['tauri', 'R', 'shell', 'L', 'Invokes']],
});
