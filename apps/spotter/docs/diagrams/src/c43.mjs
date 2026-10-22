import { P, save, link, hub } from './common.mjs';

// ---------- C4 level 3: engine and core ----------
{
  const d = P({ title: 'Spotter engine — components', kind: 'C4 · Level 3 — Components of spotter-engine', w: 1456,
    desc: 'One check, end to end: the scheduler or a command starts it, the pipeline reads the source, the core decides, the database remembers, the notifier tells.' });
  const W = 248, NH = 96, gx = 48, c1 = gx + 24, c2 = c1 + W + 88, c3 = c2 + W + 88, gw = c3 + W + 24 - gx, c4 = gx + gw + 96;
  const sy = 144, gy = sy + 96 + 56;
  const row = (i) => gy + 40 + i * (NH + 40);
  const gh = 40 + 4 * NH + 3 * 40 + 24;
  d.node('shell', c2, sy, W, 96, 'process', { name: 'Tauri shell', tech: 'app/src-tauri', desc: 'Calls the Engine; supplies the Notifier and SecretStore.' });
  d.group('g', gx, gy, gw, gh, 'spotter-engine [Rust library · adapters and orchestration]');
  d.node('sched', c1, row(0), W, NH, 'accent', { name: 'Scheduler', tech: 'scheduler.rs', desc: 'Every 5 s: what is due, at most N at once, then the digest.' });
  d.node('facade', c2, row(0), W, NH, 'accent', { name: 'Engine facade', tech: 'engine.rs', desc: 'One entry point per UI command; owns watches and states.' });
  d.node('cfg', c3, row(0), W, NH, 'accent', { name: 'Config store', tech: 'config_store.rs', desc: 'spotter.toml: atomic writes, backup, damage tolerated.' });
  d.node('check', c2, row(1), W, NH * 2 + 40, 'accent', { name: 'Check', tech: 'check.rs',
    desc: 'One check of one watch: fetch, select, soak, compare with the state, record events, decide what to notify, queue digests.' });
  d.node('notif', c1, row(1), W, NH, 'accent', { name: 'Notifier', tech: 'trait · the shell implements it', desc: 'notify and run_actions; webhook and ntfy go out from the engine.' });
  d.node('sec', c1, row(2), W, NH, 'accent', { name: 'SecretStore', tech: 'trait · the keyring implements it', desc: 'get, set, delete by scope and key; never logged.' });
  d.node('db', c3, row(1), W, NH, 'accent', { name: 'Database', tech: 'db.rs · SQLite', desc: 'States, events, validators, digest queue; recovers a corrupt file.' });
  d.node('pipe', c2, row(3), W, NH, 'accent', { name: 'Pipeline', tech: 'pipeline.rs', desc: 'Plan, then folder, polite HTTP or page renderer, then parse.' });
  d.node('fetch', c1, row(3), W, NH, 'accent', { name: 'Polite fetcher', tech: 'fetch.rs · guard.rs', desc: 'Per-host gap, ETag, caps, redirects; loopback and link-local refused.' });
  d.node('rport', c3, row(3), W, NH, 'core', { name: 'PageRenderer', tech: 'port in spotter-core', desc: 'spotter-render implements it with Chromium.' });
  d.node('core', c4, row(2), W, NH, 'core', { name: 'spotter-core', tech: 'Rust library, no I/O', desc: 'select, soak, on_success, next_delay_secs, notification policy.' });
  link(d, 'shell', 'B', 'facade', 'T', { label: 'Commands', at: 0.25 });
  link(d, 'sched', 'R', 'facade', 'L', { label: 'Due checks' });
  link(d, 'facade', 'R', 'cfg', 'L', { label: 'Config' });
  link(d, 'facade', 'B', 'check', 'T', { label: 'Runs' });
  hub(d, 'check', 'L', 'notif', 'R', { label: 'Notifies' });
  hub(d, 'check', 'L', 'sec', 'R', { label: 'Secrets' });
  hub(d, 'check', 'R', 'db', 'L', { label: 'Saves' });
  hub(d, 'check', 'R', 'core', 'L', { label: 'Decides with' });
  link(d, 'check', 'B', 'pipe', 'T', { label: 'Reads' });
  link(d, 'pipe', 'L', 'fetch', 'R', { label: 'Fetches' });
  link(d, 'pipe', 'R', 'rport', 'L', { label: 'Renders' });
  d.legend([{ kind: 'process', label: 'Caller' }, { kind: 'accent', label: 'Engine component' }, { kind: 'core', label: 'spotter-core' }, { kind: 'line', label: 'Uses' }]);
  save('c4-3-engine', d, gy + gh);
}
