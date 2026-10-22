import { P, save, sequence } from './common.mjs';

/** parts: [id, name, tech, role]; script items: ['m', a, b, text, reply?] ['s', p, text] ['o', kind, cond] ['e', cond] ['c'] ['n', p, text, w?] */
function seq(file, { title, desc, parts, script, pw = 160, gap = 40 }) {
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
  const names = { ui: 'UI', accent: 'Pitcrew', core: 'Core library', process: 'Process · OS', external: 'External' };
  d.legend([...[...new Set(parts.map((p) => p[3]))].map((k) => ({ kind: k, label: names[k] })), { kind: 'line', label: 'Call' }, { kind: 'dashline', label: 'Reply' }]);
  save(file, d, y);
}

seq('uml-seq-register-runner', {
  title: 'Register or adopt a runner', desc: 'The wizard adds the runner; the service installs the runner binary if needed, then registers with the token or adopts a registered folder.',
  parts: [['w', 'Register wizard', 'Angular UI', 'ui'], ['sh', 'Rust shell', 'Tauri commands', 'accent'], ['su', 'Supervisor', 'pitcrew-service', 'accent'], ['wk', 'Worker thread', 'work.rs', 'accent'], ['rn', 'gitea-runner', 'register, then run', 'process'], ['gt', 'Gitea / Forgejo', 'server', 'external'], ['cs', 'Credential store', 'OS', 'process']],
  script: [
    ['m', 'w', 'sh', '`register_runner(request)`'],
    ['s', 'sh', 'Parse the token (1–256 visible characters), the name and the folder'],
    ['m', 'sh', 'su', '`Status`'], ['m', 'su', 'sh', '`Status { runners, service.data_dir }`', 1],
    ['o', 'opt', '[the runner is not known yet]'], ['m', 'sh', 'su', '`AddRunner { runner }`'], ['s', 'su', 'Validate; save pitcrew.toml atomically, copy to .bak first'], ['m', 'su', 'sh', '`Ok`', 1], ['c'],
    ['m', 'sh', 'su', '`Register { runner, serverUrl, token, displayName }`'],
    ['s', 'su', 'Check the server URL and display name; remember the server, never the token'],
    ['m', 'su', 'wk', 'Start worker; the reply is held'],
    ['s', 'wk', 'Create the runner folder'],
    ['o', 'opt', '[managed binary is missing]'], ['s', 'wk', 'Discover the latest release, download, verify, swap'], ['c'],
    ['o', 'alt', '[folder already registered]'], ['s', 'wk', '`.runner` exists: adopt the folder, do not send the token'],
    ['e', '[folder not registered yet]'],
    ['m', 'wk', 'rn', '`register --no-interactive …`'], ['m', 'rn', 'gt', 'Register the runner (HTTPS)'], ['m', 'gt', 'rn', 'Registration, written to `.runner`', 1], ['m', 'rn', 'wk', 'exit 0', 1], ['c'],
    ['m', 'wk', 'su', '`Done::Registered(result)`', 1],
    ['s', 'su', 'registered = true; the controller answers `Action::Launch`'],
    ['m', 'su', 'rn', 'Start the runner process'],
    ['m', 'su', 'sh', '`Ok`', 1],
    ['o', 'opt', '[“remember token” was ticked]'], ['m', 'sh', 'cs', '`set(Pitcrew registration, server, token)`'], ['c'],
    ['m', 'sh', 'w', '`Ok`', 1],
    ['n', 'su', 'A failed registration is answered with a message that says what to fix; the log line is redacted. Status changes reach the UI through the push subscription.', 400],
  ],
});

seq('uml-seq-stall-restart', {
  title: 'Stall detection and restart', desc: 'A runner can stay alive while its polls keep timing out. Six timed-out polls in 15 minutes with no job running is a stall: the runner is stopped gracefully and restarted.',
  parts: [['rn', 'Runner process', 'gitea-runner', 'process'], ['su', 'Supervisor', 'pitcrew-service', 'accent'], ['cl', 'Log classifier', 'logline.rs', 'core'], ['ct', 'Controller', 'controller.rs', 'core'], ['sd', 'Stall detector', 'watchdog.rs', 'core'], ['rt', 'Restart tracker', 'restart.rs', 'core'], ['pc', 'Process control', 'launch.rs', 'accent']],
  script: [
    ['m', 'rn', 'su', 'Output line: “fetching a task timed out”'],
    ['m', 'su', 'cl', '`classify(line)`'], ['m', 'cl', 'su', '`FetchTimedOut`', 1],
    ['m', 'su', 'ct', '`Input::Log(FetchTimedOut)`'], ['m', 'ct', 'sd', '`record_timeout(now)`'],
    ['s', 'sd', 'Remember the first failure: status becomes Degraded'],
    ['o', 'loop', '[every second, until the verdict is Stalled]'],
    ['m', 'su', 'ct', '`Input::JobRunning(false)`, `Input::Tick`'], ['m', 'ct', 'sd', '`evaluate(now, job_running)`'], ['m', 'sd', 'ct', '`Stalled { timeouts }`', 1],
    ['o', 'opt', '[a job is running]'], ['s', 'ct', 'Verdict SlowButBusy: wait, never restart a busy runner'], ['c'],
    ['c'],
    ['m', 'ct', 'su', '`Record(Stalled)`, `StopGracefully`', 1],
    ['s', 'su', 'History: Stalled; notify; life = `Stopping { then: StallRestart }`'],
    ['m', 'su', 'pc', '`stop_graceful(runner)`'], ['m', 'pc', 'rn', 'SIGTERM · Ctrl+Break to the process group'], ['m', 'rn', 'su', 'Process exits', 1],
    ['m', 'su', 'ct', '`Input::Exited { code }`'], ['m', 'ct', 'rt', '`on_exit(started, now, Stalled)`'], ['m', 'rt', 'ct', '`RestartAt { at, delay }` or `CrashLoop { until }`', 1],
    ['s', 'ct', 'life = `WaitingToStart { at }`; history: Exited (expected)'],
    ['m', 'su', 'ct', '`Input::Tick` (now ≥ at)'], ['m', 'ct', 'su', '`Launch`', 1],
    ['m', 'su', 'pc', '`start(runner)`'], ['m', 'pc', 'rn', 'Spawn with piped output'],
    ['m', 'su', 'ct', '`Input::Launched`'],
    ['s', 'ct', 'life = Running; detector reset; history: Started'],
    ['n', 'ct', 'No graceful exit within the stop timeout (120 s) and no job running: the process tree is terminated (`Action::Kill`). A runner that exits again and again enters a crash loop.', 400],
  ],
});

seq('uml-seq-subscription', {
  title: 'App to service push subscription', desc: 'The app does not poll. One long-lived connection receives status and notifications as they happen; every command opens a short connection of its own.',
  parts: [['st', 'Angular stores', 'BACKEND token', 'ui'], ['ev', 'Events thread', 'events.rs', 'accent'], ['tr', 'Tray', 'tray.rs', 'accent'], ['ip', 'IPC server', 'ipc.rs', 'accent'], ['su', 'Supervisor', 'pitcrew-service', 'accent'], ['os', 'OS notifications', 'notification plugin', 'process']],
  script: [
    ['m', 'ev', 'st', '`pitcrew://event`: `Connection { Connecting }`'],
    ['m', 'ev', 'ip', 'Connect (named pipe or Unix socket)'], ['m', 'ev', 'ip', '`Hello { protocol: 2 }`'], ['m', 'ip', 'ev', '`Hello { protocol, serviceVersion }`', 1],
    ['m', 'ev', 'st', '`Connection { Connected { serviceVersion } }`'], ['m', 'ev', 'tr', '`update(runners, connected)`'],
    ['m', 'ev', 'ip', '`Subscribe`'], ['m', 'ip', 'ev', '`Ok`', 1],
    ['m', 'ip', 'su', '`Msg::Subscribe(sender)`'], ['m', 'su', 'ip', '`Event(Status { runners, service })` at once', 1],
    ['o', 'loop', '[on every change of a runner status or machine sample]'],
    ['m', 'su', 'ip', '`Event(Status)`, only when it changed', 1], ['m', 'ip', 'ev', 'Length-prefixed JSON frame', 1],
    ['s', 'ev', 'Menu key changed? Rebuild the tray menu; keep the latest status'],
    ['m', 'ev', 'tr', '`update(runners, true)`'], ['m', 'ev', 'st', '`BackendEvent::Status { status }`'],
    ['s', 'st', '`StatusStore.patchState`; dashboard and runners update'], ['c'],
    ['o', 'opt', '[a notification is due, on transitions only]'], ['m', 'su', 'ip', '`Event(Notification { … })`', 1], ['m', 'ip', 'ev', 'Frame', 1], ['m', 'ev', 'os', '`show(title, body)`'], ['m', 'ev', 'st', '`BackendEvent::Notification`'], ['c'],
    ['o', 'alt', '[the connection is lost]'], ['s', 'ev', 'Connection closed: service stopped or restarted'], ['m', 'ev', 'tr', '`update([], false)`'], ['m', 'ev', 'st', '`Connection { Unavailable }`: banner'],
    ['s', 'ev', 'Sleep 1 s, doubling to 15 s; reset once connected again'], ['m', 'ev', 'ip', 'Connect and Hello again'], ['c'],
    ['n', 'su', 'A service with another protocol version answers `ProtocolMismatch` and the UI says to install the same Pitcrew version for both. Commands such as Start still work over their own short connections.', 360],
  ],
});

seq('uml-seq-update-fetch', {
  title: 'Safe update — find, download and verify', desc: 'Part 1 of 2. The service looks for a newer release, downloads it next to the installed binary and checks it against two hosts. Nothing has changed on disk yet.',
  pw: 192, gap: 64,
  parts: [['su', 'Supervisor', 'pitcrew-service', 'accent'], ['um', 'Update machine', 'pitcrew-core', 'core'], ['wk', 'Worker thread', 'work.rs', 'accent'], ['rh', 'Release hosts', 'dl.gitea.com, gitea.com', 'external']],
  script: [
    ['m', 'su', 'wk', 'Check for a release (on schedule or on request)'], ['m', 'wk', 'rh', '`version.json`'], ['m', 'rh', 'wk', 'Latest version', 1], ['m', 'wk', 'su', '`Done::Checked(version)`', 1],
    ['s', 'su', 'Policy decides `Apply { target }`: automatic, inside the window, newer, not skipped'],
    ['m', 'su', 'um', '`Begin { plan }`'], ['m', 'um', 'su', '`[Download { version }]`', 1],
    ['m', 'su', 'wk', 'Download to `<binary>.download`'], ['m', 'wk', 'rh', '`GET` binary (HTTPS, ≤ 256 MB)'], ['m', 'rh', 'wk', 'Bytes; SHA-256 computed while streaming', 1], ['m', 'wk', 'su', '`Done::Downloaded`', 1],
    ['m', 'su', 'um', '`Downloaded`'], ['m', 'um', 'su', '`[Verify]`', 1],
    ['m', 'su', 'wk', 'Verify'], ['m', 'wk', 'rh', '`<file>.sha256` and `checksums.txt`'],
    ['s', 'wk', 'Both checksums must agree and match; run `--version` with a timeout'],
    ['m', 'wk', 'su', '`Done::Verified`', 1],
    ['m', 'su', 'um', '`Verified`'],
    ['n', 'um', 'Continues in “Safe update: swap, restart, confirm”. A failed download or verification ends the update as Failed; the installed binary is untouched.', 380],
  ],
});

seq('uml-seq-update-swap', {
  title: 'Safe update — swap, restart and confirm', desc: 'Part 2 of 2. Wait for the running job, swap the binary, restart, confirm; roll back automatically if the new runner is not accepted. A running job is never killed.',
  pw: 192, gap: 56,
  parts: [['su', 'Supervisor', 'pitcrew-service', 'accent'], ['um', 'Update machine', 'pitcrew-core', 'core'], ['ct', 'Controller', 'pitcrew-core', 'core'], ['wk', 'Worker thread', 'work.rs', 'accent'], ['rn', 'Runner process', 'gitea-runner', 'process']],
  script: [
    ['m', 'um', 'su', '`[WatchForIdle]` (phase: Draining)', 1], ['m', 'su', 'ct', '`Hold(true)`: nothing restarts by itself'],
    ['o', 'loop', '[until the runner is idle]'], ['s', 'su', 'Every tick: no job running? Drain timeout 3 h'], ['c'],
    ['m', 'su', 'um', '`RunnerIdle`'], ['m', 'um', 'su', '`[StopGracefully]` (phase: Stopping)', 1],
    ['m', 'su', 'ct', '`StopNow`'], ['m', 'ct', 'rn', 'SIGTERM · Ctrl+Break to the process group'], ['m', 'rn', 'su', 'Exit (`RunnerStopped`)', 1],
    ['m', 'su', 'um', '`RunnerStopped`'], ['m', 'um', 'su', '`[Swap]` (phase: Swapping)', 1],
    ['m', 'su', 'wk', 'Swap'], ['s', 'wk', 'Binary → `<binary>.previous`; download → binary (atomic renames)'], ['m', 'wk', 'su', '`Done::Swapped`', 1],
    ['m', 'su', 'um', '`Swapped`'], ['m', 'um', 'su', '`[StartRunner]` (phase: Confirming)', 1],
    ['m', 'su', 'ct', '`LaunchNow`'], ['m', 'ct', 'rn', 'Start the new binary'], ['m', 'rn', 'su', 'Log: declared successfully (the server accepted it)', 1],
    ['o', 'alt', '[the new runner is accepted]'], ['m', 'su', 'um', '`Accepted`'], ['m', 'um', 'su', '`[Finish { Updated }]`', 1],
    ['e', '[it exits early, or is not accepted within 5 minutes]'],
    ['m', 'su', 'um', '`RunnerExited` / `Tick`'], ['m', 'um', 'su', '`[StopRunner, RestorePrevious]` (phase: RollingBack)', 1],
    ['m', 'su', 'wk', 'Restore the previous binary'], ['m', 'wk', 'su', '`Done::RolledBack`', 1], ['m', 'su', 'ct', '`LaunchNow`: the old version runs again'],
    ['m', 'su', 'um', '`RolledBack`'], ['m', 'um', 'su', '`[Finish { RolledBack { reason } }]`', 1], ['c'],
    ['m', 'su', 'ct', '`Hold(false)`; history; notification'],
    ['n', 'um', 'Postponed: a job that outlasts the drain timeout, or a runner that does not stop in 2 minutes, ends the update as Postponed; the download is discarded.', 400],
  ],
});
