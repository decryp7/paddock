import { P, save, cls } from './common.mjs';
const Y = 144;

// ---------- the ports ----------
{
  const d = P({ title: 'Ports and their adapters', kind: 'UML · Class diagram', w: 1456,
    desc: 'The core and the engine reach the outside world only through these traits: real adapters on the right, test doubles on the left.' });
  const W = 392, GAP = 88, c1 = 48, c2 = c1 + W + GAP, c3 = c2 + W + GAP; // doubles | ports | adapters
  // middle column: the ports
  cls(d, 'rs', c2, Y, W, { name: 'ReleaseSource', stereotype: 'trait · spotter-core', role: 'core', ops: ['list(limit): Result<Vec<Candidate>, SourceError>'] });
  cls(d, 'pr', c2, Y + d.rect.rs[3] + 56, W, { name: 'PageRenderer', stereotype: 'trait · spotter-core', role: 'core', ops: ['render(url, wait_selector, timeout):', '  Result<RenderedPage, RenderError>'] });
  cls(d, 'no', c2, d.rect.pr[1] + d.rect.pr[3] + 56, W, { name: 'Notifier', stereotype: 'trait · spotter-engine', role: 'accent', ops: ['notify(&Notification)', 'run_actions(&ActionRequest)'] });
  cls(d, 'ss', c2, d.rect.no[1] + d.rect.no[3] + 56, W, { name: 'SecretStore', stereotype: 'trait · spotter-engine', role: 'accent', ops: ['get(scope, key): Option<String>', 'set(scope, key, value): Result<(), String>', 'delete(scope, key): Result<(), String>'] });
  cls(d, 'ck', c2, d.rect.ss[1] + d.rect.ss[3] + 56, W, { name: 'Clock', stereotype: 'trait · spotter-engine', role: 'accent', ops: ['now(): Timestamp', 'utc_offset_minutes(): i32'] });
  // column 2: real adapters; column 3: test doubles
  const at = (id) => d.rect[id][1];
  cls(d, 'fc', c3, at('rs'), W, { name: 'FolderChecker · HttpChecker', stereotype: 'spotter-sources', attrs: ['one per plan: a folder or an HTTP request', 'NeedsRenderer: explains a missing browser'] });
  cls(d, 'cr', c3, at('pr'), W, { name: 'ChromiumRenderer', stereotype: 'spotter-render', attrs: ['supervised Chromium over CDP'] });
  cls(d, 'sn', c3, at('no'), W, { name: 'ShellNotifier', stereotype: 'app/src-tauri', attrs: ['OsToast, opener, actions'] });
  cls(d, 'ks', c3, at('ss'), W, { name: 'KeyringSecrets', stereotype: 'app/src-tauri', attrs: ['OS credential store'] });
  cls(d, 'sc', c3, at('ck'), W, { name: 'SystemClock', stereotype: 'spotter-engine', attrs: ['real time; the shell sets the offset'] });
  cls(d, 'fr', c1, at('pr'), W, { name: 'FakeRenderer', stereotype: 'engine tests', attrs: ['canned pages and errors'] });
  cls(d, 'nn', c1, at('no'), W, { name: 'NullNotifier · RecordingNotifier', stereotype: 'spotter-engine', attrs: ['nothing · records for asserts'] });
  cls(d, 'ms', c1, at('ss'), W, { name: 'MemorySecrets', stereotype: 'spotter-engine', attrs: ['in-memory map'] });
  cls(d, 'mc', c1, at('ck'), W, { name: 'ManualClock', stereotype: 'spotter-engine', attrs: ['time moves when a test says so'] });
  // realisation: dashed, hollow head at the port, straight through the middle of the overlap
  const impl = (port, from, fromSide) => {
    const [px, py, pw, ph] = d.rect[port], [fx, fy, fw, fh] = d.rect[from];
    const yy = Math.round((Math.max(py, fy) + Math.min(py + ph, fy + fh)) / 2);
    const left = fromSide === 'R'; // the implementor sits left of the port
    d.edge(from, port, [left ? fx + fw : fx, yy], [left ? px : px + pw, yy], { dash: true, arrow: 'blockThin' });
  };
  impl('rs', 'fc', 'L'); impl('pr', 'cr', 'L'); impl('no', 'sn', 'L'); impl('ss', 'ks', 'L'); impl('ck', 'sc', 'L');
  impl('pr', 'fr', 'R'); impl('no', 'nn', 'R'); impl('ss', 'ms', 'R'); impl('ck', 'mc', 'R');
  d.legend([{ kind: 'core', label: 'Port in spotter-core' }, { kind: 'accent', label: 'Port or type in the engine, shell or adapters' }, { kind: 'dashline', label: 'Implements' }]);
  save('uml-class-ports', d, Math.max(...Object.values(d.rect).map(([, y, , h]) => y + h)));
}
