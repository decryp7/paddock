import { P, save, sequence } from './common.mjs';

/** parts: [id, name, tech, role]; script items: ['m', a, b, text, reply?] ['s', p, text] ['o', kind, cond] ['e', cond] ['c'] ['n', p, text, w?] */
function seq(file, { title, desc, parts, script, pw = 168, gap = 40 }) {
  const w = 96 + parts.length * pw + (parts.length - 1) * gap;
  const d = P({ title, kind: 'UML · Sequence diagram', desc, w });
  const s = sequence(d, parts.map(([id, name, tech, role]) => ({ id, name, tech, role })), { pw, gap });
  for (const [k, ...a] of script) {
    if (k === 'm') s.msg(a[0], a[1], a[2], { reply: !!a[3] });
    else if (k === 's') s.self(a[0], a[1]);
    else if (k === 'o') s.open(a[0], a[1]);
    else if (k === 'e') s.else(a[0]);
    else if (k === 'c') s.close();
    else if (k === 'n') s.note(a[0], a[1], a[2]);
  }
  const y = s.done();
  const names = { ui: 'Angular UI', accent: 'Dyno', core: 'Pure core', process: 'Process · OS · data', plugin: 'Plugin', external: 'External' };
  d.legend([...[...new Set(parts.map((p) => p[3]))].map((k) => ({ kind: k, label: names[k] })), { kind: 'line', label: 'Call' }, { kind: 'dashline', label: 'Reply' }]);
  save(file, d, y);
}

seq('uml-seq-act-undo', {
  title: 'Act on a process, then undo',
  desc: 'Every action is checked against the protected list, applied to a ProcessRef (pid plus start time) and recorded with its inverse, so Activity can undo it.',
  parts: [['ui', 'Angular UI', 'processes page', 'ui'], ['sh', 'Tauri shell', 'app/src-tauri', 'accent'], ['h', 'Agent handlers', 'dyno-agent', 'accent'], ['pr', 'protect', 'dyno-core', 'core'], ['ct', 'ProcessControl', 'dyno-platform', 'accent'], ['db', 'Store', 'SQLite', 'process']],
  script: [
    ['m', 'ui', 'sh', 'confirm dialog, then `act(target, action)`'],
    ['m', 'sh', 'h', '`Request::Act { target, action }`'],
    ['s', 'h', '`action.validate(cores)`'],
    ['m', 'h', 'pr', '`reason(process)`'], ['m', 'pr', 'h', '`None`: allowed', 1],
    ['m', 'h', 'ct', '`apply(target, action)`'],
    ['s', 'ct', 'start time still matches? else `NotFound`'],
    ['m', 'ct', 'h', '`Ok`', 1],
    ['m', 'h', 'db', 'put `ActivityEntry` with `UndoRecord` (inverse)'],
    ['m', 'h', 'sh', '`Reply::Activity(entry)`', 1],
    ['m', 'sh', 'ui', 'toast with Undo; entry in Activity', 1],
    ['m', 'ui', 'sh', '`undo(activity_id)`'], ['m', 'sh', 'h', '`Request::Undo`'],
    ['m', 'h', 'ct', '`apply(inverse)`'],
    ['m', 'h', 'db', 'mark entry undone'],
    ['m', 'h', 'sh', '`Reply::Activity(entry)`', 1],
  ],
});

seq('uml-seq-agent-restart', {
  title: 'Agent restart after an app update',
  desc: 'A newer app asks the older agent to exit and the service manager starts the new one. What rules and caps applied is released first, and is recoverable after a crash.',
  parts: [['up', 'Updater', 'Tauri updater', 'accent'], ['ap', 'App', 'Tauri shell', 'accent'], ['old', 'Old agent', 'dyno-agent', 'accent'], ['sv', 'Service manager', 'launchd · systemd · Run', 'process'], ['new', 'New agent', 'dyno-agent', 'accent']],
  script: [
    ['m', 'up', 'ap', 'new version installed; relaunch'],
    ['m', 'ap', 'old', 'read `agent.json`: version, pid, token'],
    ['m', 'ap', 'old', 'connect, `Hello`; versions differ'],
    ['m', 'ap', 'old', '`Request::Shutdown`'],
    ['s', 'old', '`release_all()`; remove `agent.json`'],
    ['m', 'old', 'sv', 'process exits'],
    ['m', 'sv', 'new', '`KeepAlive` / `Restart=always` starts the new binary'],
    ['m', 'ap', 'sv', '`dyno-agent service ensure` (re-register if stale)'],
    ['s', 'new', '`release_all()` from `process-control.json`; open store; migrate'],
    ['s', 'new', 'write `agent.json`, listen'],
    ['m', 'ap', 'new', 'connect and `Subscribe`'],
    ['m', 'new', 'ap', '`Push(LiveUpdate)`', 1],
    ['n', 'sv', 'Windows has no manager that restarts the agent: the app starts it (`dyno-agent run`) when `agent.json` is stale; the Run entry does it at the next logon.', 400],
  ],
});

seq('uml-seq-helper-install', {
  title: 'First admin action installs the helper',
  desc: 'The helper is installed only on a click in the install dialog, after the OS\'s own prompt. Then the daemon authenticates the agent, validates each action again and writes an audit line.',
  parts: [['ui', 'Angular UI', 'dialog', 'ui'], ['ag', 'dyno-agent', 'user process', 'accent'], ['os', 'OS prompt', 'UAC · password · polkit', 'process'], ['hi', 'dyno-helper install', 'elevated, once', 'process'], ['hp', 'dyno-helper serve', 'system service', 'process']],
  script: [
    ['m', 'ui', 'ag', '`Request::ApplyTuning` (needs administrator rights)'],
    ['m', 'ag', 'ui', 'preview and "install the helper first"', 1],
    ['m', 'ui', 'ag', 'Confirm in the install dialog; `Request::InstallHelper`'],
    ['m', 'ag', 'os', 'elevate one command (`runas`, password dialog, `pkexec`)'],
    ['m', 'os', 'ag', 'approved or declined', 1],
    ['m', 'ag', 'hi', '`install --user --agent-exe`'],
    ['s', 'hi', 'copy itself to a system folder, write token and config, register and start the service'],
    ['m', 'ag', 'ui', '`Reply::Helper` (installing); poll `HelperStatus`', 1],
    ['m', 'ag', 'hp', 'connect; token, protocol, peer user and agent path'],
    ['m', 'hp', 'ag', '`Info { version }`', 1],
    ['m', 'ag', 'hp', '`HelperRequest::Apply(HelperAction)`'],
    ['s', 'hp', 'validate again, apply, read the setting back, audit line'],
    ['m', 'hp', 'ag', 'applied or failed', 1],
    ['m', 'ag', 'ui', '`Reply::Activity` with undo', 1],
    ['n', 'ag', 'Declined: observe mode continues and nothing changes.', 320],
  ],
});

seq('uml-seq-history-rollup', {
  title: 'History: raw points, rollups and a range query',
  desc: 'Raw points every 5 seconds; closed buckets become minute, 15-minute and hour rows with min, average and max, so a short spike stays visible at every zoom level.',
  parts: [['sm', 'Sampler', 'agent thread', 'accent'], ['wr', 'HistoryWriter', 'rollup.rs', 'accent'], ['db', 'Store', 'SQLite, WAL', 'process'], ['hh', 'History handler', 'dyno-agent', 'accent'], ['ui', 'Angular UI', 'HistoryStore', 'ui']],
  script: [
    ['m', 'sm', 'wr', '`record(sample, groups)` each tick'],
    ['m', 'wr', 'db', '`put_point(Raw)` every 5 s; per-app minute accumulates'],
    ['m', 'wr', 'db', 'flush app minutes when a minute closes'],
    ['s', 'wr', 'end of `record`: `roll_up(store, now)`'],
    ['m', 'wr', 'db', '`points(source tier, bucket)` for each closed bucket'],
    ['s', 'wr', '`downsample`: min / avg / max'],
    ['m', 'wr', 'db', '`put_point(Minute | QuarterHour | Hour)`; set meta'],
    ['m', 'wr', 'db', '`prune(retention)` hourly, small transactions'],
    ['m', 'ui', 'hh', '`Request::History { range, end }`'],
    ['s', 'hh', 'finest tier that covers the range'],
    ['m', 'hh', 'db', 'read points'], ['m', 'db', 'hh', 'points (at most a few thousand)', 1],
    ['m', 'hh', 'ui', '`Reply::History` with events and boots', 1],
  ],
});

seq('uml-seq-live-update', {
  title: 'Live update push',
  desc: 'The app subscribes once; the sampler measures at adaptive intervals and the agent pushes each LiveUpdate to every subscriber. A slow subscriber loses frames.',
  parts: [['ui', 'Angular UI', 'LiveStore', 'ui'], ['sh', 'Tauri shell', 'app/src-tauri', 'accent'], ['sv', 'Agent server', 'dyno-ipc', 'accent'], ['sm', 'Sampler', 'agent thread', 'accent'], ['co', 'dyno-core', 'pure', 'core'], ['os', 'OS adapters', 'dyno-platform', 'accent']],
  script: [
    ['m', 'ui', 'sh', '`subscribe(processes)`'],
    ['m', 'sh', 'sv', '`Request::Subscribe { detail }`'],
    ['s', 'sv', 'set detail, wake sampler'],
    ['m', 'sv', 'sh', '`Reply::Ok` and the latest update at once', 1],
    ['s', 'sm', 'wait `intervals(settings, load)`'],
    ['m', 'sm', 'os', '`sample(now)`, `processes()`'],
    ['m', 'os', 'sm', '`SystemSample`, `Vec<ProcessSample>`', 1],
    ['m', 'sm', 'co', '`group`, `diagnose`, `Engine::tick`'],
    ['m', 'co', 'sm', 'groups, causes, `Commands`', 1],
    ['m', 'sm', 'sv', '`LiveUpdate` (trimmed per subscriber)'],
    ['m', 'sv', 'sh', '`Message::Live` push; bounded queue, dropped if slow'],
    ['m', 'sh', 'ui', 'Tauri event with the update'],
    ['s', 'ui', 'LiveStore sets signals; charts redraw'],
  ],
});
