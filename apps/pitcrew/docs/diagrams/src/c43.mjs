import { P, save, link, hub } from './common.mjs';
const Y = 144;
const core = (id, d, x, y, w, h, name, tech, desc, role = 'core') => d.node(id, x, y, w, h, role, { name, tech, desc });
const LEG_CORE = [{ kind: 'core', label: 'Component (pure Rust)' }, { kind: 'line', label: 'Feeds' }];

// ---- core: supervision of one runner ----
{
  const d = P({ title: 'Pitcrew — core: supervising a runner', kind: 'C4 · Level 3 — Components of pitcrew-core', w: 1200,
    desc: 'Pure functions and state machines: no I/O, no clock, no OS. The service feeds events in and carries out the actions that come out.' });
  const NH = 112, a = 72, b = a + 264 + 120, c = b + 288 + 120;
  const g = (i) => Y + 40 + i * (NH + 40);
  const H = 40 + NH * 3 + 80 + 24;
  d.group('g', 48, Y, 1104, H, 'Supervision of one runner [controller and its rules]');
  core('cl', d, a, g(0), 264, NH, 'Log classifier', 'logline.rs', 'Turns each runner log line into an event: declared, fetch timed out, task started, not registered.');
  core('jt', d, a, g(1), 264, NH, 'Job tracker', 'jobs.rs', 'A job runs while its processes do, plus 20 s grace; reads repository, job and run from task-started lines.');
  core('se', d, a, g(2), 264, NH, 'Session rules', 'session.rs', 'Where a runner may start: user\'s session, service session, or wait for log-on; when to relocate.');
  core('ct', d, b, g(0), 288, NH * 3 + 80, 'Controller', 'controller.rs', 'One runner\'s life: Input in, Actions out. Stop, restart, drain and stall restarts wait for the job to end.');
  core('wd', d, c, g(0), 264, NH, 'Watchdog', 'watchdog.rs', 'StallDetector: at least 6 timed-out polls in 15 minutes while no job runs means stalled.');
  core('rp', d, c, g(1), 264, NH, 'Restart policy', 'restart.rs', 'Back-off 10 s doubling to 5 min, reset after 10 min; crash loop after 5 exits in 10 min.');
  core('he', d, c, g(2), 264, NH, 'Health', 'health.rs', 'RunnerStatus: one word from the facts (Idle, Busy, Draining, Stalled, CrashLoop, Updating).');
  hub(d, 'ct', 'L', 'cl', 'R', { rev: true, label: 'Events' });
  hub(d, 'ct', 'L', 'jt', 'R', { rev: true, label: 'Job state' });
  hub(d, 'ct', 'L', 'se', 'R', { rev: true, label: 'Log-on' });
  hub(d, 'ct', 'R', 'wd', 'L', { label: 'Timeouts' });
  hub(d, 'ct', 'R', 'rp', 'L', { label: 'On exit' });
  hub(d, 'ct', 'R', 'he', 'L', { label: 'Facts' });
  d.legend(LEG_CORE);
  save('c4-3-core-supervision', d, Y + H);
}
// ---- core: updates ----
{
  const d = P({ title: 'Pitcrew — core: updates', kind: 'C4 · Level 3 — Components of pitcrew-core', w: 1344,
    desc: 'Where versions come from, who may update when, and the machine that does it. The service carries out its commands.' });
  const NH = 120, W = 248, gap = 96, x = (i) => 72 + i * (W + gap), g = (i) => Y + 40 + i * (NH + 40);
  const H = 40 + NH * 2 + 40 + 24;
  d.group('g', 48, Y, 24 + 4 * W + 3 * gap + 24, H, 'Updates [ADR 0002]');
  core('loc', d, x(0), g(0), W, NH, 'Release locations', 'distribution.rs', 'RunnerKind: where each kind publishes versions, binaries, checksums and signatures.');
  core('ver', d, x(0), g(1), W, NH, 'Versions', 'version.rs', 'RunnerVersion: parsed once, ordered, never a bare string.');
  core('chk', d, x(1), g(0), W, NH, 'Checksums', 'checksum.rs', 'Parses checksum files; SHA-256 verify; two hosts must agree.');
  core('pol', d, x(1), g(1), W, NH, 'Update policy', 'update/policy.rs', 'Automatic, notify only or manual; window; channel; pin and skip.');
  core('mac', d, x(2), g(0), W, NH * 2 + 40, 'Update machine', 'update/machine.rs', 'Downloading, Verifying, Draining, Stopping, Swapping, Confirming, RollingBack: commands out, an outcome at the end.');
  core('out', d, x(3), g(0), W, NH, 'Outcomes', 'update/machine.rs', 'Updated, RolledBack, Postponed, Cancelled, Failed; kept as the last update.');
  link(d, 'loc', 'R', 'chk', 'L', { label: 'URLs' });
  hub(d, 'mac', 'L', 'chk', 'R', { rev: true, label: 'Verified' });
  hub(d, 'mac', 'L', 'pol', 'R', { rev: true, label: 'Begin' });
  link(d, 'ver', 'R', 'pol', 'L', { label: 'Ordered by' });
  hub(d, 'mac', 'R', 'out', 'L', { label: 'Finish' });
  d.legend(LEG_CORE);
  save('c4-3-core-updates', d, Y + H);
}
// ---- core: records and helpers ----
{
  const d = P({ title: 'Pitcrew — core: records and helpers', kind: 'C4 · Level 3 — Components of pitcrew-core', w: 1464,
    desc: 'Housekeeping, metrics and the versioned records, then the small shared helpers (also used by the app).' });
  const W = 232, NH = 112;
  let y = Y;
  const strip = (id, label, items, links) => {
    d.group(id, 48, y, 24 + items.length * W + (items.length - 1) * 96 + 24, 40 + NH + 24, label);
    items.forEach((it, i) => core(it[0], d, 72 + i * (W + 96), y + 40, W, NH, it[1], it[2], it[3]));
    links.forEach(([a, b, l]) => link(d, a, 'R', b, 'L', { label: l }));
    y += 40 + NH + 24 + 56;
  };
  strip('g1', 'Housekeeping and metrics [ADR 0006, 0007]', [
    ['hk', 'Housekeeping', 'housekeeping.rs', 'Plans a cleanup from a folder scan and a policy.'],
    ['us', 'Runner usage', 'usage.rs', 'tree_members: the runner and everything it started; CPU share from cumulative time.'],
    ['me', 'Metrics', 'metrics.rs', 'MachineSample and tiered stores: raw, 5-minute and hourly buckets.']], [['us', 'me', 'Points']]);
  strip('g2', 'Records and configuration [versioned, tolerant of damage]', [
    ['cf', 'Configuration', 'config.rs', 'pitcrew.toml: schema version, defaults, runners; a bad entry is skipped.'],
    ['hi', 'History', 'history.rs', 'Bounded event list; summaries per local day.'],
    ['st', 'Service state', 'state.rs', 'state.json: per runner history, last update, usage; versioned.']], [['hi', 'st', 'Part of']]);
  const helpers = [
    ['pr', 'Protocol', 'protocol.rs', 'Request, Response, ServiceEvent; framing; MAX_FRAME; a Secret that never prints.'],
    ['na', 'Names', 'names.rs', 'RunnerName: refuses path characters and length.'],
    ['la', 'Runner layout', 'layout.rs', 'Folder layout and register arguments per runner kind.'],
    ['lo', 'Log lines', 'logs.rs', 'LogLine, bounded plain-text search, no user patterns.'],
    ['no', 'Notifications', 'notify.rs', 'What the user is told, on transitions only.'],
    ['re', 'Redaction', 'redact.rs', 'Masks tokens and secrets in anything that leaves the service.'],
    ['ga', 'Gitea API', 'gitea_api.rs', 'URLs and parsing for job outcomes; the app does the HTTP.'],
    ['ti', 'Time', 'time.rs', 'Timestamp.'],
    ['en', 'Runner environment', 'runner_env.rs', 'The login shell\'s PATH, the fallback and merging.'],
    ['su', 'Self-update watch', 'self_update.rs', 'The service\'s program changed and nothing runs: exit and let the OS start the new one.'],
    ['sp', 'Service paths', 'service_paths.rs', 'Data folder and socket location per OS.'],
    ['ss', 'Service spec', 'service_spec.rs', 'Renders the launch agent plist and the systemd user unit.'],
    ['pl', 'Platform', 'platform.rs', 'Os and Arch, and the release names per platform.']];
  const per = 5, gw = 24 + per * W + (per - 1) * 40 + 24, rowsN = Math.ceil(helpers.length / per);
  const gh = 40 + rowsN * NH + (rowsN - 1) * 40 + 24;
  d.group('g3', 48, y, gw, gh, 'Wire and helpers [shared with the app]');
  helpers.forEach((it, i) => core(it[0], d, 72 + (i % per) * (W + 40), y + 40 + Math.floor(i / per) * (NH + 40), W, NH, it[1], it[2], it[3]));
  d.legend(LEG_CORE);
  save('c4-3-core-support', d, y + gh);
}
// ---- service: control path ----
{
  const d = P({ title: 'Pitcrew — service: control path', kind: 'C4 · Level 3 — Components of pitcrew-service', w: 1344,
    desc: 'How the service starts, how the app reaches it, and where it asks the core for decisions. The thin edge around the pure core.' });
  const W = 248, NH = 120, gap = 104, x = (i) => 48 + i * (W + gap), r = (i) => Y + 40 + i * (NH + 56);
  const gh = 40 + NH * 2 + 56 + 24;
  d.group('g', x(1) - 24, Y, 3 * W + 2 * gap + 48, gh, 'Service process');
  d.node('cli', x(1), r(0), W, NH, 'accent', { name: 'Command line', tech: 'cli.rs, main.rs', desc: 'run, install, uninstall, status, check-config; parsed once into a typed command.' });
  d.node('host', x(2), r(0), W, NH, 'accent', { name: 'Host', tech: 'host.rs', desc: 'Starts the supervisor and the IPC server; restarts the service when its own program changed.' });
  d.node('plat', x(3), r(0), W, NH, 'accent', { name: 'Platform adapters', tech: 'platform/', desc: 'Windows SCM entry; macOS launch agent; Linux systemd user unit.' });
  d.node('shell', 48 - 0, r(1), W - 24, NH, 'ui', { name: 'App shell', tech: 'separate process', desc: 'Sends requests, subscribes to events.' });
  d.node('ipc', x(1), r(1), W, NH, 'accent', { name: 'IPC server', tech: 'ipc.rs', desc: 'Named pipe with ACL or 0600 Unix socket; hello first; frames bounded; 32 connections at most.' });
  d.node('sup', x(2), r(1), W, NH, 'accent', { name: 'Supervisor', tech: 'supervisor.rs', desc: 'One thread, one channel of messages; see the next diagram.' });
  d.node('core', x(3), r(1), W, NH, 'core', { name: 'pitcrew-core', tech: 'Rust library', desc: 'Controller, update machine, policy, housekeeping, history, config, protocol.' });
  link(d, 'cli', 'R', 'host', 'L', { label: 'run' });
  link(d, 'host', 'R', 'plat', 'L', { label: 'Installs' });
  link(d, 'host', 'B', 'sup', 'T', { label: 'Spawns thread' });
  link(d, 'shell', 'R', 'ipc', 'L', { label: 'Requests' });
  link(d, 'ipc', 'R', 'sup', 'L', { label: 'Requests' });
  link(d, 'sup', 'R', 'core', 'L', { label: 'Decisions' });
  d.legend([{ kind: 'accent', label: 'Service component' }, { kind: 'core', label: 'Core library' }, { kind: 'ui', label: 'App' }, { kind: 'line', label: 'Calls' }]);
  save('c4-3-service-control', d, Y + gh);
}
// ---- service: what the supervisor drives ----
{
  const d = P({ title: 'Pitcrew — service: what the supervisor drives', kind: 'C4 · Level 3 — Components of pitcrew-service', w: 1384,
    desc: 'The supervisor decides with the core and hands everything slow or OS-specific to the parts on its right.' });
  const NH = 88, b = 48 + 264 + 104, c = b + 248 + 104, e = c + 264 + 104;
  const r = (i) => Y + i * (NH + 40);
  d.node('sup', 48, r(0), 264, 7 * NH + 6 * 40, 'accent', { name: 'Supervisor', tech: 'supervisor.rs', desc: 'One thread, one channel of messages: requests, runner output lines, worker results, log-on changes and a tick every second. Per runner it keeps a controller, a job tracker and an update machine.' });
  d.node('la', b, r(0), 248, NH, 'accent', { name: 'Launcher', tech: 'launcher.rs', desc: 'Starts a runner where it should run; a trait, so tests use a fake.' });
  d.node('pr', b, r(1), 248, NH, 'accent', { name: 'Probe', tech: 'probe.rs', desc: 'One system snapshot per tick: job processes, CPU, memory, free disk.' });
  d.node('lf', b, r(2), 248, NH, 'accent', { name: 'Runner log file', tech: 'logfile.rs', desc: 'Writes every runner line, rotates between lines, feeds the log tail.' });
  d.node('stre', b, r(3), 248, NH, 'accent', { name: 'Store', tech: 'store.rs, paths.rs', desc: 'Bounded reads; atomic writes with a .bak; the data folder per OS.' });
  d.node('le', b, r(4), 248, NH, 'accent', { name: 'Login environment', tech: 'login_env.rs', desc: 'Asks the login shell for the PATH a terminal has (5 s timeout).' });
  d.node('wk', b, r(5), 248, NH * 2 + 40, 'accent', { name: 'Worker steps', tech: 'work.rs, worker threads', desc: 'Release discovery, download, verification, binary swap and roll-back, version query, register.' });
  d.node('pc', c, r(0), 264, NH, 'accent', { name: 'Process control', tech: 'launch.rs, process.rs', desc: 'Spawns with piped output; stops gracefully, then kills.' });
  d.node('rn', e, r(0), 200, NH, 'process', { name: 'Runner process', tech: 'gitea-runner', desc: 'Its output lines flow back.' });
  d.node('ht', c, r(5), 232, NH, 'accent', { name: 'HTTPS client', tech: 'net.rs', desc: 'HTTPS only, timeouts, size caps; SHA-256 while streaming.' });
  d.node('cu', c, r(6), 232, NH, 'accent', { name: 'Cleanup', tech: 'cleanup.rs', desc: 'Deletes what the core\'s plan says; failures are reported.' });
  for (const [id, l] of [['la', 'Launch'], ['pr', 'Every 5 s'], ['lf', 'Each line'], ['stre', 'Saves state'], ['le', 'Runner start']]) hub(d, 'sup', 'R', id, 'L', { label: l });
  hub(d, 'sup', 'R', 'wk', 'L', { label: 'Starts' });
  link(d, 'la', 'R', 'pc', 'L', { label: 'Spawns' });
  link(d, 'pc', 'R', 'rn', 'L', { label: 'Starts' });
  hub(d, 'wk', 'R', 'ht', 'L', { label: 'Fetches' });
  hub(d, 'wk', 'R', 'cu', 'L', { label: 'Results' });
  d.legend([{ kind: 'accent', label: 'Service component' }, { kind: 'process', label: 'Runner process' }, { kind: 'line', label: 'Calls' }]);
  save('c4-3-service-supervisor', d, r(6) + NH);
}
