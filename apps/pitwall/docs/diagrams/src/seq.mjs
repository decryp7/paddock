import { P, save, sequence } from './common.mjs';
import { ld, tw } from '../../../../../diagrams/tools/lib.mjs';

/**
 * parts: [id, name, tech, role]; script items: ['m', a, b, text, reply?] ['s', p, text] ['o', kind, cond] ['e', cond] ['c'] ['n', p, text, w?]
 * ['b', p] starts and ['x', p] ends an activation that no call or reply bounds (the caller of the whole sequence).
 * Activation bars: a call that gets a reply from the callee draws a bar on the callee between the two.
 */
function seq(file, { title, desc, parts, script, pw = 152, gap }) {
  gap ??= Math.min(160, Math.floor((1464 - parts.length * pw) / (parts.length - 1)));
  const w = 96 + parts.length * pw + (parts.length - 1) * gap;
  const d = P({ title, kind: 'UML · Sequence diagram', desc, w });
  const s = sequence(d, parts.map(([id, name, tech, role]) => ({ id, name, tech, role })), { pw, gap, selfLeftOnLast: true });
  const idx = Object.fromEntries(parts.map((p, i) => [p[0], i]));
  const px = (id) => 48 + idx[id] * (pw + gap) + pw / 2;
  const stack = {}; const bars = []; const held = {}; let arrow;
  for (const [k, ...a] of script) {
    if (k === 'm') {
      const span = Math.abs(idx[a[0]] - idx[a[1]]) * (pw + gap) - 24;
      if (tw(a[2], 12) + 8 > span) console.warn(`${file}: label "${a[2]}" is ${tw(a[2], 12)} px, room ${span}`);
      const y0 = s.y;
      arrow = y0 + 24;
      s.msg(a[0], a[1], a[2], { reply: !!a[3] });
      if (!a[3]) (stack[a[1]] ??= []).push(y0 + 24);
      else {
        const st = stack[a[0]];
        if (st?.length) bars.push([a[0], st.length - 1, st.pop(), s.y - 10]);
      }
    } else if (k === 's') s.self(a[0], a[1], a[2]);
    else if (k === 'b') held[a[0]] = arrow ?? s.y;
    else if (k === 'x') bars.push([a[0], 0, held[a[0]], s.y - 10]);
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
  title: 'Chat request — admit and queue', desc: 'Part 1 of 2. A client calls a standard API; Pitwall checks who it is, which model or alias it means, what it may do, whether the engine build can serve it, and waits for a turn (ADR 0003, 0005, 0007, 0014, 0018).',
  parts: [['cl', 'Client app', 'external', 'external'], ['ht', 'http', 'axum handlers', 'process'], ['au', 'Auth', 'apps.json', 'process'], ['ap', 'AppState', 'start_chat', 'accent'], ['co', 'pitwall-core', 'presets, limits', 'core'], ['sv', 'Serving', 'resolve, queue', 'process']],
  script: [
    ['m', 'cl', 'ht', '`POST /v1/chat/completions`'],
    ['s', 'ht', 'Gate: Host is loopback, Origin allowed'],
    ['m', 'ht', 'au', '`authenticate(credential, …)`'], ['m', 'au', 'ht', '`Principal`', 1],
    ['s', 'ht', 'Parse once into the neutral `ChatRequest`'],
    ['m', 'ht', 'ap', '`start_chat(who, meta, request)`'], ['b', 'ap'],
    ['m', 'ap', 'sv', '`resolve(model, vision, tools)`'],
    ['s', 'sv', 'A model name wins; else a preset alias; else `auto:…`; else default'],
    ['m', 'sv', 'co', '`presets::resolve`'],
    ['m', 'co', 'sv', '`Resolution`, or 404', 1],
    ['m', 'sv', 'ap', '`Resolved { id, chosen_by, preset }`', 1],
    ['m', 'ap', 'co', '`presets::apply(preset, req)`'],
    ['s', 'co', 'Fills temperature, top-p, max tokens, system prompt only where the request set none'],
    ['m', 'ap', 'co', '`AppPermissions::authorize(…)`'],
    ['m', 'co', 'ap', '`Priority`, or 403', 1],
    ['m', 'ap', 'au', '`admit(app, per_min, now)`'],
    ['m', 'au', 'ap', 'Ok, or 429', 1],
    ['m', 'ap', 'co', '`check_budget(used, limits)`'],
    ['m', 'co', 'ap', 'Ok, or 429 with the limit and reset', 1],
    ['m', 'ap', 'sv', '`check_capabilities(resolved, needed)`'],
    ['s', 'sv', 'Matrix::gate for the pinned or automatic build of that model'],
    ['m', 'sv', 'ap', 'Ok, or the gate error', 1],
    ['m', 'ap', 'sv', '`acquire(app, model, priority, kind, app_cap)`'],
    ['s', 'sv', 'Wait: global, per-model (its slots) and per-app caps, aging'],
    ['m', 'sv', 'ap', '`Permit`', 1], ['x', 'ap'],
    ['n', 'ap', 'Continues in “Chat request: load and answer”. A closed connection cancels the job and frees the slot. Gate errors: engine_build_too_old (503), capability_unsupported (400), engine_pin_unavailable (503); a stale alias is preset_target_unavailable (404).', 400],
  ],
});

seq('uml-seq-chat-serve', {
  title: 'Chat request — load and answer', desc: 'Part 2 of 2. If the model is not in memory it is loaded first, with its slots; the answer streams back in the client’s own format and the usage is recorded (ADR 0003, 0006, 0013, 0016, 0019).',
  parts: [['cl', 'Client app', 'external', 'external'], ['ap', 'AppState', 'start_chat', 'accent'], ['sv', 'Serving', 'hot models', 'process'], ['co', 'pitwall-core', 'fit, memory', 'core'], ['lc', 'LlamaCpp', 'Engine adapter', 'process'], ['ls', 'llama-server', 'child process', 'process'], ['us', 'Usage', 'usage.json', 'core']],
  script: [
    ['b', 'ap'],
    ['m', 'ap', 'sv', '`ensure_for(resolved)`'],
    ['o', 'alt', '[the model is not in memory: cold start]'],
    ['m', 'sv', 'co', '`need_tokens(size, n)`'],
    ['m', 'sv', 'co', '`plan_load(id, need, …)`'],
    ['m', 'co', 'sv', 'Proceed, evict, wait, refuse', 1],
    ['m', 'sv', 'lc', '`start(&Launch)`'],
    ['s', 'lc', '`--ctx-size` = context x slots; `--parallel` = slots'],
    ['s', 'lc', 'Build: model pin, global pin, else automatic'],
    ['m', 'lc', 'ls', 'Spawn on `127.0.0.1`'],
    ['m', 'lc', 'ls', '`/health`, then `/props`'],
    ['m', 'ls', 'lc', '`Running { port, props }`', 1],
    ['m', 'lc', 'sv', '`Running`', 1],
    ['c'],
    ['m', 'sv', 'ap', '`(Busy, cold_start_ms)`', 1],
    ['m', 'ap', 'co', '`check_memory(footprint, total, share)`'],
    ['m', 'ap', 'sv', '`chat(busy, permit, …)`'],
    ['s', 'sv', 'Slot affinity: the conversation’s slot if free, else idle longest, else none'],
    ['m', 'sv', 'ls', '`POST /v1/chat/completions` (stream, `id_slot`)'],
    ['m', 'ls', 'sv', 'Events, token by token (`cache_n`)', 1],
    ['m', 'sv', 'ap', 'Events', 1],
    ['m', 'ap', 'cl', 'Tokens in the client’s format', 1],
    ['m', 'ap', 'us', '`record(UsageEvent)`'],
    ['s', 'us', 'Hourly and daily buckets per app; saved every 30 s'], ['x', 'ap'],
    ['n', 'ap', 'The reply carries X-Pitwall-Model, -Alias (when a preset was asked for), -Engine, -Chosen-By and -Cold-Start-Ms; the permit is released and last_used is set.', 400],
  ],
});

seq('uml-seq-chat-cache', {
  title: 'Chat request — response cache and slot affinity', desc: 'After the gate, a deterministic request from an opted-in app or preset is looked up before it is queued: a hit needs no model and no queue. A miss runs on the slot that served the conversation before and the completed answer is stored off the request path (ADR 0019).',
  parts: [['cl', 'Client app', 'external', 'external'], ['ap', 'AppState', 'start_chat', 'accent'], ['co', 'pitwall-core', 'cache keys', 'core'], ['ca', 'Cache store', 'memory, cache.sqlite', 'process'], ['sv', 'Serving', 'queue, slots', 'process'], ['ls', 'llama-server', 'child process', 'process']],
  script: [
    ['m', 'cl', 'ap', '`POST /v1/chat/completions`'],
    ['s', 'ap', 'Gate, alias, limits and capability check as in “admit and queue”'],
    ['m', 'ap', 'co', '`cache::key(model, request)`'],
    ['s', 'co', 'Eligible: temperature 0 or a seed, no tools, app or preset opted in, no Cache-Control: no-cache', 190],
    ['m', 'co', 'ap', '`Key`, or bypass', 1],
    ['o', 'alt', '[hit: stored and not expired]'],
    ['m', 'ap', 'ca', '`get(key, now)`'],
    ['m', 'ca', 'ap', 'Answer, hit count + 1', 1],
    ['m', 'ap', 'cl', 'Answer, `X-Pitwall-Cache: hit`', 1],
    ['e', '[miss]'],
    ['m', 'ap', 'sv', '`acquire`, `ensure_for` (queue, load)'],
    ['s', 'sv', 'Slot affinity: the conversation’s slot if free, else idle longest, else none'],
    ['m', 'sv', 'ls', '`/v1/chat/completions` (`id_slot`)'],
    ['m', 'ls', 'sv', 'Events and `timings.cache_n`', 1],
    ['m', 'sv', 'ap', 'Events', 1],
    ['m', 'ap', 'cl', 'Answer, `X-Pitwall-Cache: miss`', 1],
    ['m', 'ap', 'ca', '`put(key, answer)` (only a completed answer)'],
    ['s', 'ca', 'Writer thread saves it; least-recently-used entries are dropped when the cache is over its size cap'],
    ['c'],
    ['n', 'ap', 'Usage records cacheHits, cacheMisses, prefixTokens (cache_n) and savedTokens. A hit is not queued, loads no model and is recorded as a request.', 400],
  ],
});

seq('uml-seq-engine-install', {
  title: 'Installing an engine with self-test and rollback', desc: 'A pinned llama.cpp build is downloaded, checked, unpacked defensively and run once before it replaces anything; a failed self-test leaves the previous build active (ADR 0004).',
  parts: [['wi', 'Pitwall window', 'Angular UI', 'ui'], ['mg', 'mgmt', 'install_build', 'process'], ['lc', 'LlamaCpp', 'install()', 'accent'], ['eg', 'Egress', 'only HTTP client', 'process'], ['gh', 'GitHub releases', 'external', 'external'], ['en', 'Data folder', 'engines/, partial/', 'process'], ['nw', 'New llama-server', 'build under test', 'process']],
  script: [
    ['m', 'wi', 'mg', '`install_build(buildId)`'],
    ['s', 'mg', 'The install runs in the background and the Download is shown at once'],
    ['m', 'mg', 'lc', '`install(id, …)`'],
    ['m', 'lc', 'en', 'Create `engines/<id>.new`'],
    ['m', 'lc', 'eg', '`download(url, sha256)`'], ['m', 'eg', 'gh', '`GET` (resumes a partial file)'], ['m', 'gh', 'eg', 'Bytes', 1],
    ['s', 'eg', 'SHA-256 matches the pinned catalog, else delete and fail'],
    ['m', 'eg', 'lc', 'Verified file', 1],
    ['s', 'lc', 'Unpack: flat names, expected files, size caps, safe links'],
    ['m', 'lc', 'nw', '`--version` (30 s), then `--list-devices`'],
    ['o', 'alt', '[the self-test passes]'],
    ['m', 'nw', 'lc', 'Devices, for example MTL0', 1],
    ['m', 'lc', 'en', 'Write `build.json`; move the old build to `.old`, `.new` becomes `<id>`, delete `.old`'],
    ['m', 'lc', 'mg', 'Done', 1],
    ['s', 'mg', 'Remember the previous build for rollback; event “Installed llama.cpp”'],
    ['e', '[it fails or times out]'],
    ['m', 'lc', 'en', 'Delete the stage; the previous build stays'],
    ['m', 'lc', 'mg', 'Error: self-test failed', 1],
    ['c'],
    ['m', 'mg', 'wi', 'Live update: Done', 1],
  ],
});

seq('uml-seq-engine-pin', {
  title: 'Pinning and rolling back an engine build', desc: 'A build can be pinned for every model or for one; a pin is strict, a change never swaps a running engine, and features the build cannot serve are refused with a reason (ADR 0018).',
  parts: [['wi', 'Pitwall window', 'Angular UI', 'ui'], ['mg', 'mgmt', '/mgmt/v1/rpc', 'process'], ['ep', 'engine_pins', 'pitwall-server', 'accent'], ['rt', 'runtimes::pins', 'pitwall-core', 'core'], ['lc', 'LlamaCpp', 'set_pins', 'process'], ['df', 'Data folder', 'engine-pins.json', 'process']],
  script: [
    ['m', 'wi', 'mg', '`set_model_pin(modelId, …)`'],
    ['m', 'mg', 'ep', '`set_model(model, build)`'],
    ['s', 'ep', 'Parse the `BuildId`; take the pins lock'],
    ['m', 'ep', 'rt', '`set_model(…, installed)`'],
    ['s', 'rt', 'Strict: installed and self-test passed, else `PinError`'],
    ['m', 'rt', 'ep', 'Pins, or the error to show', 1],
    ['m', 'ep', 'df', '`save_pins`: atomic write, `.bak` kept'],
    ['m', 'ep', 'lc', '`set_pins(pins)`'],
    ['m', 'ep', 'rt', '`pins::moved(running, pins, …)`'],
    ['m', 'rt', 'ep', 'Models that keep their build for now', 1],
    ['m', 'ep', 'mg', '`PinChange`', 1],
    ['m', 'mg', 'wi', 'Pinned, applies on next load', 1],
    ['o', 'opt', '[the user rolls back]'],
    ['m', 'wi', 'mg', '`rollback_build()`'],
    ['m', 'mg', 'ep', '`rollback(st)`'],
    ['m', 'ep', 'rt', '`rollback_target(…)`'],
    ['m', 'rt', 'ep', 'Target, or why not', 1],
    ['m', 'ep', 'df', 'Pin it for all models; save'],
    ['m', 'ep', 'mg', '`PinChange`', 1],
    ['m', 'mg', 'wi', 'Result and message', 1],
    ['c'],
    ['n', 'lc', '`PinChange` carries appliesOnNextLoad: a running model keeps its build; `start` picks one with `pins::select` (model pin, global pin, automatic) on the next load. A request needing vision, tools or JSON-schema output is checked first: engine_build_too_old (503), capability_unsupported (400).', 420],
  ],
});

seq('uml-seq-idle-release', {
  title: 'Idle release of a model', desc: 'Every five seconds Pitwall asks the pure memory policy what to give back and carries it out; a model that is answering is never unloaded (ADR 0006).',
  parts: [['tk', 'Tick loop', 'every 5 s', 'process'], ['sv', 'Serving', 'hot models', 'accent'], ['me', 'memory', 'pitwall-core', 'core'], ['sy', 'system', 'RAM and GPU probes', 'process'], ['ls', 'llama-server', 'child process', 'process'], ['ev', 'Live feed', 'dashboard, tray', 'process']],
  script: [
    ['m', 'tk', 'sv', '`tick()`'],
    ['s', 'sv', 'Forget engines whose process exited; schedule restarts'],
    ['m', 'sv', 'sy', '`read_system(hardware)`'], ['m', 'sy', 'sv', 'Total, available, pressure', 1],
    ['m', 'sv', 'me', '`plan_release(now, hot, …)`'],
    ['m', 'me', 'sv', '`Unload` or `ReleaseContext` actions', 1],
    ['o', 'loop', '[for each action on an idle model]'],
    ['m', 'sv', 'ls', 'Context: erase the slots; unload: drop the handle, the child is killed'],
    ['m', 'sv', 'ev', 'Event “Released <model> (Idle)”; models pushed'],
    ['c'],
    ['n', 'me', 'Rules in order: busy or loading models are never released; pressure releases least recently used first; then idle time; then deep idle; yield shortens idle times while a full-screen app runs. Half way to idle the context is cleared and the weights stay.', 440],
  ],
});

seq('uml-seq-model-download', {
  title: 'Downloading a model', desc: 'The fit check comes before the download; the file is fetched from an allow-listed host, resumed if interrupted, verified against Hugging Face’s SHA-256 and checked to be a GGUF; its origin is recorded beside it (ADR 0001, 0006, 0016, 0017).',
  parts: [['wi', 'Pitwall window', 'Angular UI', 'ui'], ['mg', 'mgmt', '/mgmt/v1', 'process'], ['lb', 'Library', 'models', 'accent'], ['eg', 'Egress', 'only HTTP client', 'process'], ['hf', 'Hugging Face', 'external', 'external'], ['df', 'Data folder', 'models/', 'process']],
  script: [
    ['m', 'wi', 'mg', '`repo_files(repo)`'], ['m', 'mg', 'lb', '`repo(repo, context, room)`'],
    ['m', 'lb', 'eg', '`get_json(tree, info)`'], ['m', 'eg', 'hf', 'File list with sizes, LFS SHA-256'], ['m', 'hf', 'eg', 'Answer', 1],
    ['m', 'lb', 'mg', 'Variants with fit and slots', 1], ['m', 'mg', 'wi', 'Fits, Tight or Won’t fit', 1],
    ['m', 'wi', 'mg', '`download_model(repo, file)`'], ['m', 'mg', 'lb', '`download(repo, variant)`'],
    ['s', 'lb', 'Every URL parsed as `DownloadUrl`'],
    ['m', 'lb', 'mg', 'Download id, state Queued', 1],
    ['o', 'loop', '[per file of the variant]'],
    ['m', 'lb', 'eg', '`download(url, .part, sha256)`'], ['m', 'eg', 'hf', '`GET` with Range'], ['m', 'hf', 'eg', '206 or 200 with bytes', 1],
    ['m', 'eg', 'df', 'Append to `<file>.gguf.part`'],
    ['s', 'eg', 'SHA-256 must equal the expected one, else delete'],
    ['s', 'lb', 'Verifying: parse the GGUF header, else discard'],
    ['m', 'lb', 'df', 'Rename `.part` to `models/<repo>/<file>.gguf`'],
    ['c'],
    ['m', 'lb', 'df', 'Write `<file>.gguf.origin.json`: repo, size, SHA-256s, commit'],
    ['m', 'lb', 'wi', 'State Done, live update', 1],
  ],
});

seq('uml-seq-update-check', {
  title: 'Model update notices', desc: 'Opt-in and check only: Pitwall asks Hugging Face about repositories you already downloaded from and tells you about a newer file or a better quantisation. It never downloads by itself (ADR 0017).',
  parts: [['tk', 'Schedule', 'first look 60 s, then 5 min', 'process'], ['ap', 'AppState', 'run_update_check', 'accent'], ['up', 'Updates', 'check()', 'process'], ['hs', 'HfSource', 'RepoSource', 'process'], ['eg', 'Egress', 'only HTTP client', 'process'], ['hf', 'Hugging Face', 'external', 'external'], ['wi', 'Pitwall window', 'Angular UI', 'ui']],
  script: [
    ['m', 'tk', 'ap', '`scheduled_update_tick()`'],
    ['o', 'opt', '[opt-in is on, the schedule is due]'],
    ['m', 'ap', 'up', '`check(source, Request)`'],
    ['s', 'up', 'One run at a time; at most 40 repositories'],
    ['o', 'loop', '[each repository, 30 s limit]'],
    ['m', 'up', 'hs', '`snapshot(repo)`'],
    ['m', 'hs', 'eg', '`get_json` tree and info'],
    ['m', 'eg', 'hf', 'check only, opt-in'],
    ['m', 'hf', 'eg', 'Listing and commit', 1], ['m', 'eg', 'up', 'Snapshot', 1],
    ['c'],
    ['s', 'up', '`evaluate`: newer file, better quant, imatrix; save `model-updates.json`'],
    ['m', 'up', 'ap', '`Report { fresh }`', 1],
    ['m', 'ap', 'wi', 'Event “Model update”, at most 3'],
    ['m', 'wi', 'ap', '`update_notices()`'],
    ['c'],
    ['o', 'opt', '[the user chooses Get it]'],
    ['m', 'wi', 'ap', '`download_model(repo, file, …)`'],
    ['n', 'up', 'The normal download flow, after the user confirms the host and size. Dismiss hides one offer; Remind me snoozes it 1 to 90 days.', 400],
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
    ['m', 'wi', 'au', '`approve_pairing` > `decide_pairing`'],
    ['s', 'au', 'New key: `pw_` + 32 random bytes; keep its SHA-256 only'],
    ['m', 'ap', 'ht', '`GET /x/v1/pair/{id}`'],
    ['m', 'ht', 'ap', '`{ status: approved, key }`, once', 1],
    ['m', 'ap', 'ht', '`GET /x/v1/pair/{id}` again'],
    ['m', 'ht', 'ap', '`{ status: approved, key: null }`', 1],
    ['n', 'ap', 'The app keeps the key in the OS credential store and sends `Bearer pw_…` from now on.', 340],
  ],
});

seq('uml-seq-cli', {
  title: 'The pitwall command line', desc: 'The CLI is a client of the local management and standard APIs (ADR 0015): no new server surface, no network beyond 127.0.0.1.',
  parts: [['ow', 'Machine owner', 'Person', 'process'], ['cl', 'pitwall CLI', 'terminal process', 'accent'], ['di', 'discovery.json', 'data folder', 'process'], ['ht', 'mgmt · http', 'axum, gate', 'process'], ['sv', 'Serving', 'hot models', 'process']],
  script: [
    ['m', 'ow', 'cl', '`pitwall load qwen2.5-7b`'],
    ['m', 'cl', 'di', 'Read the port (else `--port`, else 11435)'],
    ['m', 'di', 'cl', '`port`', 1],
    ['s', 'cl', 'Admin key: `--key-file`, `PITWALL_ADMIN_KEY`, OS store, `admin.key`'],
    ['m', 'cl', 'ht', '`POST /mgmt/v1/rpc/load_model { id }`'],
    ['s', 'ht', 'Gate: loopback Host, admin key'],
    ['m', 'ht', 'sv', '`ensure(id)`'],
    ['m', 'sv', 'ht', '`(Busy, cold_start_ms)`', 1],
    ['m', 'ht', 'cl', 'ok', 1],
    ['m', 'cl', 'ow', '“Loaded <model>.”, exit code 0', 1],
    ['o', 'opt', '[`pitwall chat <model or alias>`]'],
    ['m', 'cl', 'ht', '`POST /v1/chat/completions` · `PITWALL_API_KEY`'],
    ['m', 'ht', 'cl', 'Streamed tokens, printed as they arrive', 1],
    ['c'],
    ['o', 'alt', '[nothing listens on the port]'],
    ['m', 'cl', 'ow', 'How to start Pitwall; exit code 3', 1],
    ['e', '[memory, busy or a limit refuses]'],
    ['m', 'cl', 'ow', 'What to change; exit code 4', 1],
    ['c'],
    ['n', 'cl', 'Exit codes: 0 ok, 1 error, 2 usage, 3 not running, 4 refused. The CLI never starts a model host of its own; keys are never command-line arguments.', 380],
  ],
});
