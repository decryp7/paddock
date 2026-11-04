import { MockWidgets, mockMetricReading, mockWidgetOptionsFromUrl } from './widgets';

/** A `Storage` in memory (the unit tests never touch the real one). */
function memory(): Storage {
  const m = new Map<string, string>();
  return { getItem: (k) => m.get(k) ?? null, setItem: (k, v) => void m.set(k, v), removeItem: (k) => void m.delete(k), clear: () => m.clear(), key: () => null, length: 0 } as Storage;
}

const popover = (w: MockWidgets) => ('popover' in w.view('popover') ? (w.view('popover') as { popover: { layout: { placements: { id: string; span: number; widget: unknown }[] }; canUndo: boolean } }).popover : null)!;
const ids = (w: MockWidgets) => popover(w).layout.placements.map((p) => p.id);

describe('MockWidgets layouts', () => {
  it('starts with the popup defaults and edits them, with undo and redo', () => {
    const w = new MockWidgets();
    expect(ids(w)).toEqual(['active-alarms', 'disk-space', 'cpu-memory']);
    w.edit({ popover: { add: { widget: 'network', span: null, at: 0 } } });
    expect(ids(w)).toEqual(['network', 'active-alarms', 'disk-space', 'cpu-memory']);
    expect(popover(w).canUndo).toBe(true);
    w.edit({ popover: { move: { id: 'network', to: 3 } } });
    expect(ids(w).at(-1)).toBe('network');
    w.edit({ popover: { remove: { id: 'network' } } });
    expect(popover(w).canUndo).toBe(true);
    w.edit({ popover: 'undo' });
    expect(ids(w)).toContain('network');
    w.edit({ popover: 'redo' });
    expect(ids(w)).not.toContain('network');
    w.edit({ popover: 'undo' });
    w.edit({ popover: 'resetToDefaults' });
    expect(ids(w)).toEqual(['active-alarms', 'disk-space', 'cpu-memory']);
  });

  it('refuses what the engine refuses, saying what to change', () => {
    const w = new MockWidgets();
    expect(() => w.edit({ popover: { add: { widget: 'cpuMemory', span: null, at: null } } })).toThrow(/only 1 time/);
    expect(() => w.edit({ popover: { resize: { id: 'cpu-memory', span: 1 } } })).toThrow(/not available/);
    expect(() => w.edit({ popover: { remove: { id: 'nope' } } })).toThrow(/no longer/);
    expect(() => w.edit({ popover: 'undo' })).toThrow(/nothing to undo/);
    for (const k of ['network', 'backupHealth', 'lastIssue', 'pinnedCounters'] as const) w.edit({ popover: { add: { widget: k, span: null, at: null } } });
    w.edit({ popover: { add: { widget: { diskSpace: { volume: 'system' } }, span: null, at: null } } });
    expect(ids(w)).toContain('disk-space-2');
    expect(() => w.edit({ popover: { add: { widget: { diskSpace: { volume: 'all' } }, span: null, at: null } } })).toThrow(/At most 8 widgets fit in the popup/);
  });

  it('keeps layouts in storage, so a reload sees them, and tells listeners', () => {
    const storage = memory();
    const first = new MockWidgets({ storage });
    const seen: string[] = [];
    first.onLayout((s) => seen.push(s));
    first.edit({ popover: { move: { id: 'cpu-memory', to: 0 } } });
    expect(seen).toEqual(['popover']);
    expect(ids(new MockWidgets({ storage }))).toEqual(['cpu-memory', 'active-alarms', 'disk-space']);
  });

  it('applies and saves dashboard presets', () => {
    const w = new MockWidgets();
    w.edit({ dashboard: { applyPreset: { id: 'developer' } } });
    const d = (w.view('dashboard') as { dashboard: { layout: { placements: unknown[]; presets: { name: string }[] }; builtinPresets: unknown[] } }).dashboard;
    expect(d.layout.placements).toHaveLength(7);
    expect(d.builtinPresets).toHaveLength(4);
    w.edit({ dashboard: { savePreset: { name: 'Lean' } } });
    w.edit({ dashboard: { deletePreset: { id: 'custom-lean' } } });
    expect(() => w.edit({ dashboard: { deletePreset: { id: 'overview' } } })).toThrow(/cannot be deleted/);
  });
});

describe('MockWidgets dashboard grid', () => {
  type Dash = { revision: number; canUndo: boolean; canRedo: boolean; outcome: { position: number; of: number } | null; layout: { placements: { id: string; x: number; y: number; w: number; h: number }[] }; arrangements: { columns: number; placements: { id: string }[] }[]; moves: { id: string; moves: { left: boolean } }[] };
  const dash = (w: MockWidgets): Dash => (w.view('dashboard') as unknown as { dashboard: Dash }).dashboard;
  const rect = (w: MockWidgets, id: string) => dash(w).layout.placements.find((p) => p.id === id)!;

  it('starts with the 1.0 Overview as rectangles, with arrangements for each breakpoint', () => {
    const w = new MockWidgets();
    const d = dash(w);
    expect(d.layout.placements).toHaveLength(15);
    expect(rect(w, 'health')).toMatchObject({ x: 0, y: 0, w: 6, h: 9 });
    expect(rect(w, 'diagnosis')).toMatchObject({ x: 6, y: 0, w: 6, h: 9 });
    expect(d.arrangements.map((a) => a.columns)).toEqual([12, 6, 1]);
    const order = (n: number) => d.arrangements[n]!.placements.map((p) => p.id);
    expect(order(1)).toEqual(order(0));
    expect(order(2)).toEqual(order(0));
    expect(d.moves).toHaveLength(15);
    expect(d.moves.find((m) => m.id === 'health')!.moves.left).toBe(false);
  });

  it('counts a revision per edit and refuses an edit made on an older one as stale', () => {
    const w = new MockWidgets();
    const r = dash(w).revision;
    w.edit({ dashboard: { arrange: { gesture: { nudge: { id: 'health', dir: 'down' } } } } }, r);
    expect(dash(w).revision).toBe(r + 1);
    expect(() => w.edit({ dashboard: 'undo' }, r)).toThrow(expect.objectContaining({ code: 'stale' }));
    expect(() => w.preview({ surface: 'dashboard', revision: r, gesture: { toFirst: { id: 'cpu' } }, seq: 1 })).toThrow(expect.objectContaining({ code: 'stale' }));
  });

  it('previews without storing, and says where the widget lands', () => {
    const w = new MockWidgets();
    const r = dash(w).revision;
    const reply = w.preview({ surface: 'dashboard', revision: r, gesture: { toLast: { id: 'health' } }, seq: 7 });
    expect(reply.seq).toBe(7);
    expect(reply.view.outcome).toMatchObject({ id: 'health', position: 15, of: 15 });
    expect(dash(w).revision).toBe(r);
    expect(rect(w, 'health').y).toBe(0);
  });

  it('keeps arrangement edits undoable, redoable and bounded to twenty steps', () => {
    const w = new MockWidgets();
    for (let i = 0; i < 25; i++) w.edit({ dashboard: { arrange: { gesture: { grow: { id: 'graph', dir: i % 2 === 0 ? 'down' : 'up' } } } } });
    let steps = 0;
    while (dash(w).canUndo) {
      w.edit({ dashboard: 'undo' });
      steps += 1;
    }
    expect(steps).toBe(20);
    expect(dash(w).canRedo).toBe(true);
  });

  it('adds at the default size in the first free place and refuses spans and flow edits', () => {
    const w = new MockWidgets();
    w.edit({ dashboard: { add: { widget: { kind: 'graph', metric: 'memory', options: { rangeSecs: 300, showLabel: true, fill: true, accent: null }, thresholds: { warning: null, critical: null } }, span: null, at: null } } });
    expect(dash(w).layout.placements).toHaveLength(16);
    expect(() => w.edit({ dashboard: { resize: { id: 'cpu', span: 3 } } })).toThrow(/by dragging/);
    expect(() => w.edit({ dashboard: { move: { id: 'cpu', to: 0 } } })).toThrow(/by dragging/);
  });

  it('answers a gesture as the engine did when the layout is a golden case, else the widget just moves', () => {
    const items = [{ id: 'a', rect: { x: 0, y: 0, w: 6, h: 4 } }, { id: 'b', rect: { x: 6, y: 0, w: 6, h: 4 } }];
    const gesture = { nudge: { id: 'a', dir: 'right' as const } };
    const gridCases = [{ name: 'swap', columns: 12, widths: { min: 2, max: 12 }, heights: { min: 2, max: 14 }, items, gesture, result: { ok: [{ id: 'a', rect: items[1]!.rect }, { id: 'b', rect: items[0]!.rect }] } }];
    const w = new MockWidgets({ gridCases });
    w.useGridCase('swap');
    w.edit({ dashboard: { arrange: { gesture } } });
    expect(rect(w, 'a')).toMatchObject({ x: 6, y: 0 });
    expect(rect(w, 'b')).toMatchObject({ x: 0, y: 0 });
    expect(() => w.useGridCase('nope')).toThrow(/no golden case/);
    expect(() => w.edit({ dashboard: { arrange: { gesture: { nudge: { id: 'zzz', dir: 'right' } } } } })).toThrow(/no longer/);
  });
});

describe('MockWidgets popup values', () => {
  it('sends the engine\'s words: the calm alarms row has its next step, and the agent notice and widgets share one time', () => {
    const w = new MockWidgets({ popup: { activeAlarms: 'empty' }, now: () => Date.UTC(2026, 9, 31, 6, 30) });
    const alarms = w.values().placements.find((p) => p.id === 'active-alarms')!.state;
    expect(alarms).toMatchObject({ state: 'empty', calm: true, title: 'No active alarms', detail: 'All clear.', next: { label: 'Open alarms', action: { openPage: 'alarms' } } });
    const stopped = new MockWidgets({ agentStopped: true, now: () => Date.UTC(2026, 9, 31, 6, 30) }).values();
    const times = new Set(stopped.placements.map((p) => (p.state.state === 'ready' ? p.state.staleSince : null)).filter((t) => t !== null));
    expect(times.size).toBe(1);
    const t = new Date([...times][0]!);
    const hhmm = `${String(t.getHours()).padStart(2, '0')}:${String(t.getMinutes()).padStart(2, '0')}`;
    expect(stopped.notice?.detail).toBe(`Readings paused at ${hhmm}`);
  });

  const states = (w: MockWidgets) => Object.fromEntries(w.values().placements.map((p) => [p.id, p.state.state]));

  it('is ready by default, leaves a loading kind out and shows each state by switch', () => {
    const w = new MockWidgets({ popup: { activeAlarms: 'empty', diskSpace: 'loading', cpuMemory: 'stale' } });
    expect(states(w)).toEqual({ 'active-alarms': 'empty', 'cpu-memory': 'ready' });
    const cpu = w.values().placements.find((p) => p.id === 'cpu-memory')!.state;
    expect(cpu.state === 'ready' && cpu.staleSince !== null).toBe(true);
    w.setState('activeAlarms', 'error');
    expect(states(w)['active-alarms']).toBe('error');
  });

  it('one notice when the agent stops, with every ready value stale', () => {
    const w = new MockWidgets({ agentStopped: true });
    const v = w.values();
    expect(v.notice?.action?.action).toBe('restartAgent');
    expect(v.placements.every((p) => p.state.state !== 'ready' || p.state.staleSince !== null)).toBe(true);
    const pushed: unknown[] = [];
    w.onValues((x) => pushed.push(x));
    w.setAgentStopped(false);
    expect(pushed).toHaveLength(1);
    expect(w.values().notice).toBeNull();
  });

  it('shows the network widget empty, as the engine does when offline', () => {
    const w = new MockWidgets({ popup: { network: 'empty' } });
    w.edit({ popover: { add: { widget: 'network', span: null, at: null } } });
    expect(w.values().placements.find((p) => p.id === 'network')!.state).toMatchObject({ state: 'empty', title: 'See traffic when online', detail: 'Offline now.', next: { label: 'Open network' } });
  });

  it('reads the switches from the URL and ignores nonsense', () => {
    expect(mockWidgetOptionsFromUrl('?popup=diskSpace:error,network:bogus,nope:empty&agent=stopped')).toEqual({ popup: { diskSpace: 'error' }, agentStopped: true });
    expect(mockWidgetOptionsFromUrl('')).toEqual({ popup: {}, agentStopped: false });
  });
});

describe('mockMetricReading', () => {
  const query = (metric: string) => ({ metric, thresholds: { warning: null, critical: null } }) as unknown as Parameters<typeof mockMetricReading>[0];
  const system = { cpu: 20, memory: 50, diskBusy: 35, diskFree: 40, rxBps: 2_500_000, txBps: 800 };

  it('reads disk and network like the engine: a percentage, a rate with a fitting unit', () => {
    expect(mockMetricReading(query('diskActivity'), system)).toMatchObject({ text: '35', unit: '%', percent: 35, reason: null });
    expect(mockMetricReading(query('diskFree'), system)).toMatchObject({ text: '40', unit: '%', reason: null });
    expect(mockMetricReading(query('networkDown'), system)).toMatchObject({ text: '2.5', unit: 'MB/s', percent: null, reason: null });
    expect(mockMetricReading(query('networkUp'), system)).toMatchObject({ text: '800', unit: 'B/s' });
  });

  it('says why when the disk reports no busy time', () => {
    const r = mockMetricReading(query('diskActivity'), { ...system, diskBusy: 'Not reported here.' });
    expect(r).toMatchObject({ value: null, reason: 'Not reported here.' });
  });
});
