import { P, save, sequence } from './common.mjs';
import { ld, tw } from '../../../../paddock/diagrams/tools/lib.mjs';

/**
 * parts: [id, name, tech, role]; script items: ['m', a, b, text, reply?] ['s', p, text] ['o', kind, cond] ['e', cond] ['c'] ['n', p, text, w?]
 * Activation bars: a call that gets a reply from the callee draws a bar on the callee between the two.
 */
function seq(file, { title, desc, parts, script, pw = 152, gap }) {
  gap ??= Math.min(160, Math.floor((1464 - parts.length * pw) / (parts.length - 1)));
  const w = 96 + parts.length * pw + (parts.length - 1) * gap;
  const d = P({ title, kind: 'UML · Sequence diagram', desc, w });
  const s = sequence(d, parts.map(([id, name, tech, role]) => ({ id, name, tech, role })), { pw, gap });
  const idx = Object.fromEntries(parts.map((p, i) => [p[0], i]));
  const px = (id) => 48 + idx[id] * (pw + gap) + pw / 2;
  const stack = {}; const bars = [];
  for (const [k, ...a] of script) {
    if (k === 'm') {
      const span = Math.abs(idx[a[0]] - idx[a[1]]) * (pw + gap) - 24;
      if (tw(a[2], 12) + 8 > span) console.warn(`${file}: label "${a[2]}" is ${tw(a[2], 12)} px, room ${span}`);
      const y0 = s.y;
      s.msg(a[0], a[1], a[2], { reply: !!a[3] });
      if (!a[3]) (stack[a[1]] ??= []).push(y0 + 24);
      else {
        const st = stack[a[0]];
        if (st?.length) bars.push([a[0], st.length - 1, st.pop(), s.y - 10]);
      }
    } else if (k === 's') s.self(a[0], a[1]);
    else if (k === 'o') s.open(a[0], a[1]);
    else if (k === 'e') s.else(a[0]);
    else if (k === 'c') s.close();
    else if (k === 'n') s.note(a[0], a[1], a[2]);
  }
  const y = s.done();
  for (const [p, depth, y0, y1] of bars) {
    if (y1 - y0 < 12) continue;
    const x = px(p) - 5 + depth * 5;
    d.raw(`<mxCell id="${d.id()}" value="" style="rounded=0;strokeWidth=1;strokeColor=${ld(d.t.edge)};fillColor=${ld(d.t.process.f)};" vertex="1" parent="1"><mxGeometry x="${x}" y="${Math.round(y0)}" width="10" height="${Math.round(y1 - y0)}" as="geometry"/></mxCell>`);
  }
  const names = { ui: 'Angular UI', accent: 'Pitwall (subject)', core: 'pitwall-core', process: 'Module or process', external: 'External' };
  d.legend([...[...new Set(parts.map((p) => p[3]))].map((k) => ({ kind: k, label: names[k] })), { kind: 'line', label: 'Call' }, { kind: 'dashline', label: 'Reply' }]);
  save(file, d, y);
}

seq('uml-seq-chat-admit', {
  title: 'Chat request — admit and queue', desc: 'Part 1 of 2. A client calls a standard API; Pitwall checks who it is, what it may do, which model it means, and waits for a turn (ADR 0003, 0005, 0007).',
  parts: [['cl', 'Client app', 'external', 'external'], ['ht', 'http', 'axum handlers', 'process'], ['au', 'Auth', 'apps.json', 'process'], ['ap', 'AppState', 'start_chat', 'accent'], ['co', 'pitwall-core', 'presets, select', 'core'], ['sv', 'Serving', 'queue', 'process']],
  script: [
    ['m', 'cl', 'ht', '`POST /v1/chat/completions`'],
    ['s', 'ht', 'Gate: Host is loopback, Origin allowed'],
    ['m', 'ht', 'au', '`authenticate(credential)`'], ['m', 'au', 'ht', '`Principal`', 1],
    ['m', 'ht', 'ap', '`start_chat(who, meta, request)`'],
    ['s', 'ap', 'Parse once into the neutral `ChatRequest`'],
    ['m', 'ap', 'au', '`authorize(model, priority)`'],
    ['s', 'au', 'Rate window, daily token budget'],
    ['m', 'au', 'ap', '`Priority`, or 403 / 429', 1],
    ['m', 'ap', 'co', '`resolve(model)`'],
    ['s', 'co', 'Alias, then `auto:…`, then default'],
    ['m', 'co', 'ap', '`Resolved { id, params }`', 1],
    ['m', 'ap', 'sv', '`acquire(app, model, priority)`'],
    ['s', 'sv', 'Wait: global, per-model and per-app caps, aging'],
    ['m', 'sv', 'ap', '`Permit`', 1],
    ['n', 'ap', 'Continues in “Chat request: load and answer”. A closed connection cancels the job and frees the slot. Denials say what to change; the model is chosen by alias, auto:… or default.', 400],
  ],
});

seq('uml-seq-chat-serve', {
  title: 'Chat request — load and answer', desc: 'Part 2 of 2. If the model is not in memory it is loaded first; the answer streams back in the client’s own format and the usage is recorded (ADR 0003, 0006).',
  parts: [['cl', 'Client app', 'external', 'external'], ['ap', 'AppState', 'start_chat', 'accent'], ['sv', 'Serving', 'hot models', 'process'], ['co', 'pitwall-core', 'memory', 'core'], ['lc', 'LlamaCpp', 'Engine adapter', 'process'], ['ls', 'llama-server', 'child process', 'process'], ['us', 'Usage', 'history, metrics', 'process']],
  script: [
    ['m', 'ap', 'sv', '`ensure(model, slots)`'],
    ['m', 'sv', 'co', '`plan_load(model, slots)`'],
    ['m', 'co', 'sv', 'Proceed, evict, wait, refuse', 1],
    ['o', 'alt', '[the model is not in memory: cold start]'],
    ['m', 'sv', 'lc', '`start(launch)`: context, layers, slots'],
    ['m', 'lc', 'ls', 'Spawn on `127.0.0.1:<port>`'],
    ['m', 'lc', 'ls', '`/health`, then `/props`'],
    ['m', 'ls', 'lc', '`Running { port, props }`', 1],
    ['m', 'lc', 'sv', '`Running`', 1],
    ['c'],
    ['m', 'sv', 'ap', '`(Busy, cold_start_ms)`', 1],
    ['m', 'ap', 'sv', '`chat(busy, permit, request)`'],
    ['m', 'sv', 'ls', '`POST /v1/chat/completions` (stream)'],
    ['m', 'ls', 'sv', 'Events, token by token', 1],
    ['m', 'sv', 'ap', 'Events', 1],
    ['m', 'ap', 'cl', 'Tokens in the client’s format', 1],
    ['m', 'ap', 'us', '`record(app, tokens, timing, memory)`'],
    ['s', 'us', 'Request list, per-app counters, usage history'],
    ['n', 'ap', 'The reply carries X-Pitwall-Model, -Engine, -Chosen-By and -Cold-Start-Ms; the permit is released and last_used is set.', 400],
  ],
});

seq('uml-seq-engine-install', {
  title: 'Installing an engine with self-test and rollback', desc: 'A pinned llama.cpp build is downloaded, checked, unpacked defensively and run once before it replaces anything; a failed self-test leaves the previous build active (ADR 0004).',
  parts: [['wi', 'Pitwall window', 'Angular UI', 'ui'], ['mg', 'mgmt', 'install_build', 'process'], ['lc', 'LlamaCpp', 'install()', 'accent'], ['eg', 'Egress', 'only HTTP client', 'process'], ['gh', 'GitHub releases', 'external', 'external'], ['en', 'engines/', 'data folder', 'process'], ['nw', 'New llama-server', 'build under test', 'process']],
  script: [
    ['m', 'wi', 'mg', '`install_build(id)`'], ['m', 'mg', 'lc', '`install(id)`, live progress'],
    ['m', 'lc', 'en', 'Create `engines/<id>.new`'],
    ['m', 'lc', 'eg', '`download(url, sha256)`'], ['m', 'eg', 'gh', '`GET` (resumes a partial file)'], ['m', 'gh', 'eg', 'Bytes', 1],
    ['s', 'eg', 'SHA-256 matches the pinned catalog, else delete and fail'],
    ['m', 'eg', 'lc', 'Verified file', 1],
    ['s', 'lc', 'Unpack: flat names, expected files, size caps, safe links'],
    ['m', 'lc', 'nw', '`--version` (30 s), then `--list-devices`'],
    ['o', 'alt', '[the self-test passes]'],
    ['m', 'nw', 'lc', 'Devices, for example MTL0', 1],
    ['m', 'lc', 'en', 'Write `build.json`; swap `.new` in, keep `.old`'],
    ['m', 'lc', 'mg', 'Done; event “Installed”', 1],
    ['e', '[it fails or times out]'],
    ['m', 'lc', 'en', 'Delete the stage; the previous build stays'],
    ['m', 'lc', 'mg', 'Error: self-test failed', 1],
    ['c'],
    ['m', 'mg', 'wi', 'Result and message', 1],
  ],
});

seq('uml-seq-engine-pin', {
  title: 'Pinning and rolling back an engine build', desc: 'A build can be pinned for every model or for one; features the active build cannot do are switched off with a reason instead of failing later (ADR 0004).',
  parts: [['wi', 'Pitwall window', 'Angular UI', 'ui'], ['mg', 'mgmt', 'pin_build', 'process'], ['lc', 'LlamaCpp', 'builds, pin', 'accent'], ['rt', 'runtimes', 'pitwall-core', 'core'], ['cf', 'Settings', 'settings.toml', 'process'], ['sv', 'Serving', 'hot models', 'process']],
  script: [
    ['m', 'wi', 'mg', '`pin_build(id, scope)`'],
    ['m', 'mg', 'lc', '`pin(id, scope)`'],
    ['s', 'lc', 'The build must be installed and have passed its self-test'],
    ['m', 'lc', 'rt', '`capabilities(build)`'],
    ['m', 'rt', 'lc', '`BuildCaps { vision, tools, … }`', 1],
    ['m', 'lc', 'cf', 'Write the pin atomically, backup first'],
    ['m', 'lc', 'sv', 'Next load of the model uses the pinned build'],
    ['m', 'lc', 'mg', 'Pin and capabilities', 1],
    ['m', 'mg', 'wi', 'Pinned; unsupported features off', 1],
    ['o', 'opt', '[the user rolls back]'],
    ['m', 'wi', 'mg', '`rollback_build()`'],
    ['m', 'mg', 'lc', 'Select the previous build'],
    ['m', 'lc', 'cf', 'Write the new pin'],
    ['m', 'mg', 'wi', 'Result', 1],
    ['c'],
    ['n', 'sv', 'A model that is answering keeps its running build; the pin takes effect when it is next loaded.', 340],
  ],
});

seq('uml-seq-idle-release', {
  title: 'Idle release of a model', desc: 'Every five seconds Pitwall asks the pure memory policy what to give back and carries it out; a model that is answering is never unloaded (ADR 0006).',
  parts: [['tk', 'Tick loop', 'every 5 s', 'process'], ['sv', 'Serving', 'hot models', 'accent'], ['me', 'memory', 'pitwall-core', 'core'], ['sy', 'system', 'RAM and GPU probes', 'process'], ['ls', 'llama-server', 'child process', 'process'], ['ev', 'Live feed', 'dashboard, tray', 'process'], ['us', 'Usage', 'memory history', 'process']],
  script: [
    ['m', 'tk', 'sv', '`tick()`'],
    ['s', 'sv', 'Forget engines whose process exited'],
    ['m', 'sv', 'sy', '`read_system(hardware)`'], ['m', 'sy', 'sv', 'Total, available, pressure', 1],
    ['m', 'sv', 'me', '`plan_release(now, hot, …)`'],
    ['m', 'me', 'sv', '`Unload(model, reason)`', 1],
    ['o', 'loop', '[for each Unload action on an idle model]'],
    ['m', 'sv', 'ls', 'Drop the handle: the child is killed'],
    ['m', 'sv', 'ev', 'Event “Released <model> (Idle)”; models pushed'],
    ['c'],
    ['m', 'sv', 'us', '`sample(hot models, per-app share)`'],
    ['n', 'me', 'Rules in order: busy or loading models are never released; pressure releases least recently used first; then idle time; then deep idle; yield shortens idle times while a full-screen app runs. Half way to idle the context is cleared and the weights stay.', 440],
  ],
});

seq('uml-seq-model-download', {
  title: 'Downloading a model', desc: 'The fit check comes before the download; the file is fetched from an allow-listed host, resumed if interrupted, verified against Hugging Face’s SHA-256 and checked to be a GGUF (ADR 0001, 0006).',
  parts: [['wi', 'Pitwall window', 'Angular UI', 'ui'], ['mg', 'mgmt', '/mgmt/v1', 'process'], ['lb', 'Library', 'models', 'accent'], ['eg', 'Egress', 'only HTTP client', 'process'], ['hf', 'Hugging Face', 'external', 'external'], ['df', 'Data folder', 'models/, partial/', 'process']],
  script: [
    ['m', 'wi', 'mg', '`repo_files(repo)`'], ['m', 'mg', 'lb', '`repo(repo, context, room)`'],
    ['m', 'lb', 'eg', '`get_json(hf api URL)`'], ['m', 'eg', 'hf', 'File list with sizes, LFS SHA-256'], ['m', 'hf', 'eg', 'Answer', 1],
    ['m', 'lb', 'mg', 'Variants, each with `fit::assess`', 1], ['m', 'mg', 'wi', 'Fits, Tight or Won’t fit', 1],
    ['m', 'wi', 'mg', '`download_model(repo, variant)`'], ['m', 'mg', 'lb', '`download(repo, variant)`'],
    ['s', 'lb', 'Every URL parsed as `DownloadUrl`'],
    ['m', 'lb', 'mg', 'Download id, state Queued', 1],
    ['o', 'loop', '[per file of the variant]'],
    ['m', 'lb', 'eg', '`download(url, .part, sha256)`'], ['m', 'eg', 'hf', '`GET` with Range'], ['m', 'hf', 'eg', '206 or 200 with bytes', 1],
    ['m', 'eg', 'df', 'Append to `<file>.gguf.part`'],
    ['s', 'eg', 'SHA-256 must equal the expected one, else delete'],
    ['c'],
    ['s', 'lb', 'Verifying: parse the GGUF header, else discard'],
    ['m', 'lb', 'df', 'Rename `.part` to `models/<repo>/<file>.gguf`'],
    ['m', 'lb', 'wi', 'State Done, live update', 1],
  ],
});

seq('uml-seq-update-check', {
  title: 'Model update notices', desc: 'Pitwall tells you when Hugging Face has a newer revision or a better quantisation of an installed model. It only checks; it never downloads by itself.',
  parts: [['tk', 'Schedule', 'daily, background', 'process'], ['up', 'updates', 'library', 'accent'], ['eg', 'Egress', 'only HTTP client', 'process'], ['hf', 'Hugging Face', 'external', 'external'], ['ev', 'Live feed', 'events, dashboard', 'process'], ['wi', 'Pitwall window', 'Angular UI', 'ui']],
  script: [
    ['m', 'tk', 'up', '`check_updates()`'],
    ['o', 'opt', '[offline mode is off and the check is enabled]'],
    ['o', 'loop', '[each installed model with a known repo]'],
    ['m', 'up', 'eg', '`get_json(hf model URL)`'], ['m', 'eg', 'hf', 'Revision and file list'], ['m', 'hf', 'eg', 'Answer', 1], ['m', 'eg', 'up', 'Parsed answer', 1],
    ['s', 'up', 'Compare revision and quantisation with the installed file'],
    ['c'],
    ['m', 'up', 'ev', 'Store the notices; event “Update available”'],
    ['c'],
    ['m', 'ev', 'wi', 'Notices pushed to the UI'],
    ['o', 'opt', '[the user chooses to download]'],
    ['m', 'wi', 'up', '`download_model(repo, variant)`'],
    ['n', 'up', 'This is the normal download flow, with the fit check first. Dismissing a notice keeps it quiet until the next revision.', 380],
    ['c'],
  ],
});

seq('uml-seq-pairing', {
  title: 'Pairing an app', desc: 'An app asks to pair, the user compares a six-digit code and approves it in Pitwall, and the app receives its key exactly once (ADR 0007, 0009).',
  parts: [['ap', 'App', 'Debrief, Pitcrew, …', 'external'], ['ht', 'http', '/x/v1/pair', 'process'], ['au', 'Auth', 'apps.json', 'accent'], ['wi', 'Pitwall window', 'Angular UI', 'ui'], ['ow', 'Machine owner', 'Person', 'process']],
  script: [
    ['m', 'ap', 'ht', '`POST /x/v1/pair { name }`'],
    ['m', 'ht', 'au', '`request_pairing(name)`'],
    ['m', 'au', 'ht', '`PairingRequest { id, code }`', 1],
    ['m', 'ht', 'ap', '202 `{ pairingId, code }`', 1],
    ['m', 'au', 'wi', 'Live update: pairing request'],
    ['m', 'wi', 'ow', 'Shows the code and permissions to grant'],
    ['m', 'ow', 'wi', 'Compares codes, chooses, approves'],
    ['m', 'wi', 'au', '`approve_pairing` (admin key)'],
    ['s', 'au', 'New key: `pw_` + 32 random bytes; keep its SHA-256 only'],
    ['m', 'ap', 'ht', '`GET /x/v1/pair/{id}`'],
    ['m', 'ht', 'ap', '`{ status: approved, key }`, once', 1],
    ['m', 'ap', 'ht', '`GET /x/v1/pair/{id}` again'],
    ['m', 'ht', 'ap', '`{ status: approved, key: null }`', 1],
    ['n', 'ap', 'The app keeps the key in the OS credential store and sends `Bearer pw_…` from now on.', 340],
  ],
});

seq('uml-seq-cli', {
  title: 'The pitwall command line', desc: 'The CLI is a client of the local management and standard APIs: no new server surface, no network beyond 127.0.0.1.',
  parts: [['ow', 'Machine owner', 'Person', 'process'], ['cl', 'pitwall CLI', 'terminal process', 'accent'], ['di', 'discovery.json', 'data folder', 'process'], ['ht', 'http · mgmt', 'axum, gate', 'process'], ['sv', 'Serving', 'hot models', 'process']],
  script: [
    ['m', 'ow', 'cl', '`pitwall load qwen2.5-7b`'],
    ['m', 'cl', 'di', 'Read the port; read `admin.key` (owner-only)'],
    ['m', 'di', 'cl', '`127.0.0.1:11435`, key', 1],
    ['m', 'cl', 'ht', '`POST /mgmt/v1/rpc/load_model`'],
    ['s', 'ht', 'Gate: loopback Host, admin key'],
    ['m', 'ht', 'sv', '`ensure(model, slots)`'],
    ['m', 'sv', 'ht', '`(Busy, cold_start_ms)`', 1],
    ['m', 'ht', 'cl', 'Loaded, with memory used', 1],
    ['m', 'cl', 'ow', 'Prints the result; exit code 0', 1],
    ['o', 'opt', '[`pitwall chat`: an answer from a model]'],
    ['m', 'cl', 'ht', '`POST /v1/chat/completions` · app key'],
    ['m', 'ht', 'cl', 'Streamed tokens, printed as they arrive', 1],
    ['c'],
    ['n', 'cl', 'If the server is not running the CLI says so and how to start it; it never starts a model host of its own.', 360],
  ],
});
