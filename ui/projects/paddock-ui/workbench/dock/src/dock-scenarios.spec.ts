import { checkPlan, planLayout, restorable, type OpenView, type ViewLayout } from '@decryp7/paddock-ui';

/*
 * ADR 0011's scenario table as data: a realistic set of declared layouts at the window sizes the product supports. Each
 * pane at or above its declared minimum, the lowest priority tabbed or auto-hidden first, a resize case, and a saved
 * layout that does not fit (fitted, and back with hysteresis once there is room).
 */

const lay = (area: ViewLayout['area'], min: [number, number], pref: [number, number], priority: number, edge?: 'left' | 'right'): ViewLayout => ({
  area,
  min: { width: min[0], height: min[1] },
  pref: { width: pref[0], height: pref[1] },
  priority,
  ...(edge ? { edge } : {}),
});

const explorer: OpenView = { id: 'explorer', layout: lay('side', [300, 240], [300, 400], 80, 'left'), group: 'explorer' };
const code: OpenView = { id: 'code', layout: lay('document', [480, 240], [900, 600], 100), group: 'main', row: 'main' };
const preview: OpenView = { id: 'preview', layout: lay('document', [360, 240], [600, 600], 70), group: 'preview', row: 'main' };
const assistant: OpenView = { id: 'assistant', layout: lay('side', [320, 240], [380, 400], 90, 'right'), group: 'assistant' };
const problems: OpenView = { id: 'problems', layout: lay('analysis', [400, 160], [900, 240], 60), group: 'problems', row: 'analysis' };
const jobs: OpenView = { id: 'jobs', layout: lay('status', [300, 120], [600, 200], 20), group: 'jobs' };
const ALL = [explorer, code, preview, assistant, problems, jobs];

const SIZES = [
  ['1440x900', { width: 1440, height: 900 }],
  ['1920x1080', { width: 1920, height: 1080 }],
] as const;

describe('ADR 0011 scenarios', () => {
  for (const [name, viewport] of SIZES) {
    it(`${name}: every placed pane is at or above its declared minimum`, () => {
      const plan = planLayout(viewport, ALL);
      expect(plan.violations).toEqual([]);
      expect(plan.overflow).toBe(false);
      expect(checkPlan(plan, ALL)).toEqual([]);
      const minWidth = new Map(ALL.map((v) => [v.id, v.layout.min.width]));
      for (const column of plan.columns) {
        expect(column.width, `${column.kind}`).toBeGreaterThanOrEqual(Math.max(...column.views.map((id) => minWidth.get(id) ?? 0)));
        for (const row of column.rows) for (const cell of row.cells) expect(cell.width).toBeGreaterThanOrEqual(cell.minWidth);
      }
    });

    it(`${name}: status tools slide out instead of taking room`, () => {
      const plan = planLayout(viewport, ALL);
      expect(plan.hidden.some((h) => h.id === 'jobs')).toBe(true);
    });
  }

  it('1920x1080 keeps everything but the status tool in view', () => {
    const plan = planLayout({ width: 1920, height: 1080 }, ALL);
    expect(plan.tabbed).toEqual([]);
    expect(plan.hidden.map((h) => h.id)).toEqual(['jobs']);
  });

  it('a resize from 1920 to 1100 tabs or hides the lowest priority first and never squeezes a pane under its minimum', () => {
    const wide = planLayout({ width: 1920, height: 1080 }, ALL);
    const narrow = planLayout({ width: 1100, height: 800 }, ALL);
    expect(narrow.violations).toEqual([]);
    const gone = new Set([...narrow.tabbed.map((t) => t.id), ...narrow.hidden.map((h) => h.id)]);
    expect(gone.size).toBeGreaterThan(wide.tabbed.length + wide.hidden.length);
    expect(gone.has('code')).toBe(false);
    // Lower priority yields before higher: nothing that stays in view has a lower priority than something that yielded.
    const stays = ALL.filter((v) => !gone.has(v.id) && v.layout.area !== 'status');
    const yielded = ALL.filter((v) => gone.has(v.id) && v.layout.area !== 'status');
    for (const s of stays) for (const y of yielded.filter((o) => o.layout.area === s.layout.area)) expect(s.layout.priority, `${s.id} stays before ${y.id}`).toBeGreaterThanOrEqual(y.layout.priority);
  });

  it('a saved layout that does not fit is fitted, and the displaced views return only with the grow-back margin', () => {
    const small = { width: 900, height: 700 };
    const plan = planLayout(small, ALL);
    const displaced = [...plan.tabbed.map((t) => t.id), ...plan.hidden.map((h) => h.id)];
    expect(displaced.length).toBeGreaterThan(0);
    expect(plan.violations).toEqual([]);
    const margin = { width: 64, height: 48 };
    // Displaced views still count as open: they come back when the window has room for them plus the margin.
    expect(restorable(small, ALL, displaced, margin)).toEqual([]);
    const returns = restorable({ width: 1920, height: 1080 }, ALL, displaced, margin);
    expect(returns.length).toBeGreaterThan(0);
    expect(displaced).toContain('explorer');
    expect(returns).toContain('explorer');
  });

  it('a user-dragged size is honoured but never taken below the minimum', () => {
    const plan = planLayout({ width: 1920, height: 1080 }, [code, assistant], { sizes: { assistant: { width: 100 } } });
    const column = plan.columns.find((c) => c.views.includes('assistant'));
    expect(column?.width).toBeGreaterThanOrEqual(320);
  });
});
