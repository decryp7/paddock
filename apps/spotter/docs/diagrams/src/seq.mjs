import { P, save, sequence } from './common.mjs';

/** parts: [id, name, tech, role]; script items: ['m', a, b, text, reply?] ['s', p, text] ['o', kind, cond] ['e', cond] ['c'] ['n', p, text, w?] */
function seq(file, { title, desc, parts, script, pw = 152, gap = 32 }) {
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
  const names = { ui: 'UI', accent: 'Spotter', core: 'Pure core', process: 'Process · OS · data', external: 'External' };
  d.legend([...[...new Set(parts.map((p) => p[3]))].map((k) => ({ kind: k, label: names[k] })), { kind: 'line', label: 'Call' }, { kind: 'dashline', label: 'Reply' }]);
  save(file, d, y);
}

seq('uml-seq-check-release', {
  title: 'Scheduled check finds a new release',
  desc: 'The scheduler runs a due watch; a new release is saved, notified through the OS and the watch\'s channels, and its actions run.',
  parts: [['sc', 'Scheduler', 'scheduler.rs', 'accent'], ['en', 'Engine', 'check.rs', 'accent'], ['db', 'Database', 'spotter.db · SQLite', 'process'], ['fe', 'Polite fetcher', 'fetch.rs', 'accent'],
    ['ho', 'Release host', 'HTTP · JSON · feed', 'external'], ['co', 'spotter-core', 'select, soak, policy', 'core'], ['no', 'ShellNotifier', 'Tauri shell', 'accent'], ['ch', 'OS and channels', 'OS · webhook · ntfy', 'external']],
  script: [
    ['m', 'sc', 'en', '`check_many(due)`'],
    ['s', 'en', 'At most N checks at once; a watch already running is skipped'],
    ['m', 'en', 'db', '`cached_validators(id)`'], ['m', 'db', 'en', 'ETag, Last-Modified', 1],
    ['m', 'en', 'fe', '`get(request, validators)`'],
    ['s', 'fe', 'Per-host gap; loopback and link-local refused on every hop'],
    ['m', 'fe', 'ho', 'GET with `If-None-Match`'], ['m', 'ho', 'fe', '200 and a bounded body', 1],
    ['m', 'fe', 'en', '`Fetched::Body`', 1],
    ['m', 'en', 'co', '`parse`, `select`, `soak`, `on_success`'],
    ['m', 'co', 'en', '`Effects { notify, run_actions }`', 1],
    ['m', 'en', 'db', '`save_state`, new-release event'],
    ['o', 'opt', '[notifications active and not in quiet hours]'],
    ['m', 'en', 'no', '`notify(&Notification)`'], ['m', 'no', 'ch', 'OS notification'],
    ['m', 'en', 'ch', 'POST to each webhook or ntfy topic'],
    ['c'],
    ['o', 'opt', '[the watch has actions]'],
    ['m', 'en', 'no', '`run_actions(&ActionRequest)`'],
    ['s', 'no', 'Open, download with checksum, or run a program, with a time limit'],
    ['c'],
    ['n', 'en', 'With a digest on, the release is queued in the database and sent later as one notification. A failed delivery becomes an event, never an error for the check.', 400],
  ],
});

seq('uml-seq-rendered-page', {
  title: 'Rendered page watch and “Needs you”',
  desc: 'A page that needs JavaScript is rendered in a supervised Chromium. A login or captcha is never solved: the watch asks the user.',
  parts: [['en', 'Engine', 'check.rs', 'accent'], ['pi', 'Pipeline', 'pipeline.rs', 'accent'], ['re', 'Renderer', 'spotter-render adapter', 'accent'], ['cr', 'Chromium', 'off-screen process', 'external'],
    ['co', 'spotter-core', 'parse, state', 'core'], ['db', 'Database', 'spotter.db · SQLite', 'process'], ['no', 'ShellNotifier', 'Tauri shell', 'accent'], ['us', 'User', 'OS toast, window', 'process']],
  script: [
    ['m', 'en', 'pi', '`gather(watch, …)`'],
    ['m', 'pi', 're', '`render(url, …)`'],
    ['m', 're', 'cr', 'Start, throwaway profile'],
    ['m', 're', 'cr', 'Open the page (CDP)'],
    ['m', 'cr', 're', 'Loaded, element, quiet', 1],
    ['s', 're', 'Look at the page text and address for a login, captcha or consent'],
    ['o', 'alt', '[an ordinary page]'],
    ['m', 're', 'pi', '`RenderedPage`', 1],
    ['m', 'pi', 'co', '`parse(html)` into candidates'],
    ['m', 'co', 'en', '`Fresh`, then status Ok', 1],
    ['e', '[a challenge]'],
    ['m', 're', 'pi', '`NeedsYou(message)`', 1],
    ['m', 'pi', 'en', '`SourceError::NeedsYou`', 1],
    ['m', 'en', 'co', '`on_failure`: status becomes NeedsYou'],
    ['m', 'en', 'db', '`save_state`, needs-you event'],
    ['m', 'en', 'no', '`notify(NeedsYou)`: “<watch> needs you”'],
    ['m', 'no', 'us', 'OS notification'],
    ['s', 'us', 'Sign in or solve it, then Check now'],
    ['c'],
    ['m', 're', 'cr', 'Close, delete profile'],
    ['n', 'cr', 'Nothing is solved or bypassed. A missing browser is not a failure of the page: the watch says to install or enable one in Settings.', 400],
  ],
});
