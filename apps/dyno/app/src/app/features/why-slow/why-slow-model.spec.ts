import type { Diagnosis, Suggestion } from '../../core/api.types';
import { causeLinks, diagnosisText, fixPreview, historyLine, windowText } from './why-slow-model';

const d = (extra: Partial<Diagnosis> = {}): Diagnosis => ({
  at: 1_700_000_000_000,
  windowSecs: 55,
  headline: 'A build is busy',
  causes: [
    {
      kind: 'cpuHog',
      title: 'Compiler hogs CPU',
      explanation: 'It compiles.',
      confidence: 0.86,
      evidence: [
        { label: 'CPU', value: '90%' },
        { label: 'Next: Chrome', value: '4%' },
      ],
      suggestions: [],
      involved: [{ id: 'cc', name: 'Compiler', share: '63% CPU' }],
    },
    { kind: 'lowDiskSpace', title: 'Drive is full', explanation: 'x', confidence: 0.4, evidence: [], suggestions: [], involved: [] },
  ],
  ...extra,
});

describe('causeLinks', () => {
  it('links every kind to at least one page that exists', () => {
    const paths = new Set(['/processes', '/network', '/recorder', '/health']);
    const kinds = ['nothingWrong', 'cpuHog', 'memoryPressure', 'diskQueue', 'thermalThrottling', 'osUpdate', 'antivirusScan', 'indexing', 'lowDiskSpace', 'gpuBusy'] as const;
    for (const k of kinds) {
      const links = causeLinks(k);
      expect(links.length).toBeGreaterThan(0);
      expect(links.every((l) => paths.has(l.path))).toBe(true);
    }
    expect(causeLinks('cpuHog').map((l) => l.path)).toEqual(['/processes', '/recorder']);
    expect(causeLinks('osUpdate').map((l) => l.path)).toContain('/network');
  });
});

describe('fixPreview', () => {
  const process = (action: Suggestion extends never ? never : unknown): Suggestion =>
    ({ label: 'Do it', action: { process: { target: { pid: 7, started: 1 }, name: 'cargo', action } } }) as Suggestion;

  it('shows exactly what will change and offers undo through Activity', () => {
    const p = fixPreview(process({ setPriority: 'belowNormal' }))!;
    expect(p.details).toEqual(['Change: Priority: Below normal', 'Process: cargo (id 7)', 'It is logged in Activity, with an Undo where the change can be reversed.']);
    expect(p.danger).toBe(false);
    expect(p.message).toContain('cargo');
  });
  it('warns that ending a process cannot be undone', () => {
    const p = fixPreview(process('end'))!;
    expect(p.danger).toBe(true);
    expect(p.details.at(-1)).toContain('cannot be undone');
  });
  it('has no preview for suggestions that only navigate or wait', () => {
    for (const action of ['wait', 'openTaskManager', 'openCleanup', 'openStartup', 'restart'] as const) expect(fixPreview({ label: 'x', action })).toBeNull();
  });
});

describe('diagnosisText', () => {
  it('lists the headline, each cause with its evidence and the apps involved', () => {
    const t = diagnosisText(d());
    expect(t).toContain('A build is busy');
    expect(t).toContain('looking at the last 55 s');
    expect(t).toContain('1. Compiler hogs CPU (86% confident)');
    expect(t).toContain('   - Next: Chrome: 4%');
    expect(t).toContain('Apps involved: Compiler (63% CPU)');
    expect(t).toContain('2. Drive is full (40% confident)');
    expect(t.split('\n').filter((l) => l.includes('Apps involved'))).toHaveLength(1);
  });
});

describe('windowText and historyLine', () => {
  it('words the window in seconds, minutes, or just now', () => {
    expect(windowText(d())).toBe('Measured over the last 55 s');
    expect(windowText(d({ windowSecs: 300 }))).toBe('Measured over the last 5 min');
    expect(windowText(d({ windowSecs: 0 }))).toBe('Measured just now');
  });
  it('summarises an earlier result with how many more causes it had', () => {
    expect(historyLine(d())).toBe('A build is busy (+1 more)');
    expect(historyLine(d({ causes: [] }))).toBe('A build is busy');
    expect(historyLine(d({ causes: d().causes.slice(0, 1) }))).toBe('A build is busy');
  });
});
