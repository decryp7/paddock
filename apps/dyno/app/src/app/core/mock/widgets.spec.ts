import { MockWidgets, mockWidgetOptionsFromUrl } from './widgets';

/** A `Storage` in memory (the unit tests never touch the real one). */
function memory(): Storage {
  const m = new Map<string, string>();
  return { getItem: (k) => m.get(k) ?? null, setItem: (k, v) => void m.set(k, v), removeItem: (k) => void m.delete(k), clear: () => m.clear(), key: () => null, length: 0 } as Storage;
}

const popover = (w: MockWidgets) => ('popover' in w.view('popover') ? (w.view('popover') as { popover: { layout: { placements: { id: string; span: number; widget: unknown }[] }; canUndo: boolean } }).popover : null)!;
const ids = (w: MockWidgets) => popover(w).layout.placements.map((p) => p.id);

describe('MockWidgets layouts', () => {
  it('starts with the popup defaults and edits them, with one level of undo', () => {
    const w = new MockWidgets();
    expect(ids(w)).toEqual(['active-alarms', 'disk-space', 'cpu-memory']);
    w.edit({ popover: { add: { widget: 'network', span: null, at: 0 } } });
    expect(ids(w)).toEqual(['network', 'active-alarms', 'disk-space', 'cpu-memory']);
    expect(popover(w).canUndo).toBe(false);
    w.edit({ popover: { move: { id: 'network', to: 3 } } });
    expect(ids(w).at(-1)).toBe('network');
    w.edit({ popover: { remove: { id: 'network' } } });
    expect(popover(w).canUndo).toBe(true);
    w.edit({ popover: 'undo' });
    expect(ids(w)).toContain('network');
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

describe('MockWidgets popup values', () => {
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

  it('reads the switches from the URL and ignores nonsense', () => {
    expect(mockWidgetOptionsFromUrl('?popup=diskSpace:error,network:bogus,nope:empty&agent=stopped')).toEqual({ popup: { diskSpace: 'error' }, agentStopped: true });
    expect(mockWidgetOptionsFromUrl('')).toEqual({ popup: {}, agentStopped: false });
  });
});
