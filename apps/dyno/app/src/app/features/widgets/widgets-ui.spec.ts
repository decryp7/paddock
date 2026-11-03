import { provideRouter } from '@angular/router';
import { TestBed } from '@angular/core/testing';
import { By } from '@angular/platform-browser';
import { PdWidgetGrid } from '@decryp7/paddock-ui/widgets';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import type { Cells, DashboardWidget, MetricReading, Placement } from '../../core/generated/widgets';
import { WidgetLayouts } from '../../state/widget-layouts';
import { CountersStore } from '../../state/counters.store';
import { LiveStore } from '../../state/live.store';
import { AnimatedNumber } from './animated-number';
import { DASHBOARD_KINDS, WidgetBoard } from './widget-board';
import { WidgetView } from './widget-view';
import type { DyValue } from './dashboard-states';

/** A dashboard placement of `kind` showing `metric`, `w` columns wide. */
function place(kind: DashboardWidget['kind'], metric: DashboardWidget['metric'], w = 3): Placement<DashboardWidget, Cells> {
  return { id: `${kind}-1`, x: 0, y: 0, w, h: 4, widget: { kind, metric, options: { rangeSecs: 300, showLabel: true, fill: true, accent: null }, thresholds: { warning: null, critical: null } } };
}

/** What the board gives a body: the placement and the engine's reading of it. */
function given(kind: DashboardWidget['kind'], metric: DashboardWidget['metric'], reading: MetricReading | null = null, w = 3): DyValue {
  return { placement: place(kind, metric, w), reading };
}

function body(kind: DashboardWidget['kind'], value: DyValue | null) {
  const f = TestBed.createComponent(WidgetView);
  f.componentRef.setInput('kind', kind);
  f.componentRef.setInput('value', value);
  f.detectChanges();
  return f;
}

function env() {
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }, provideRouter([])] });
  TestBed.inject(LiveStore).receive(mock.liveUpdate('summary'));
  return mock;
}

const rd = (value: number | null, tone: MetricReading['tone'] = 'ok', extra: Partial<MetricReading> = {}): MetricReading => ({ value, text: value === null ? '—' : String(value), unit: '%', percent: value, tone, reason: null, ...extra });

describe('dy-widget', () => {
  it('colours a big number by the engine tone only, with the number still printed', () => {
    env();
    const f = body('bigNumber', given('bigNumber', 'cpuTotal', rd(95, 'bad')));
    const hero = (f.nativeElement as HTMLElement).querySelector('.hero')!;
    expect(hero.getAttribute('data-tone')).toBe('bad');
    expect(hero.textContent).toContain('95');
    f.componentRef.setInput('value', given('bigNumber', 'cpuTotal', rd(5)));
    f.detectChanges();
    expect(hero.getAttribute('data-tone')).toBe('ok');
  });

  it('draws a battery ring as a ring, as the chooser says, not as a bar', () => {
    env();
    const el = body('ring', given('ring', 'battery', rd(86))).nativeElement as HTMLElement;
    expect(el.querySelector('dy-ring')).not.toBeNull();
    expect(el.querySelector('.bar-track')).toBeNull();
  });

  it('says why a graph has no reading instead of drawing a history for it', () => {
    env();
    const f = body('graph', given('graph', 'diskActivity', rd(null, 'ok', { reason: "The disk's busy time is not reported here." })));
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('dy-sparkline')).toBeNull();
    expect(el.querySelector('.note')?.textContent).toContain('not reported here');
    f.componentRef.setInput('value', given('graph', 'diskActivity', rd(40)));
    f.detectChanges();
    expect(el.querySelector('dy-sparkline')).not.toBeNull();
    expect(el.querySelector('.note')).toBeNull();
  });

  it('is content only: no head, no card, and a skeleton twin without a value', () => {
    env();
    const ready = body('bigNumber', given('bigNumber', 'cpuTotal', rd(12))).nativeElement as HTMLElement;
    expect(ready.querySelector('header, h3, .card')).toBeNull();
    const loading = body('bigNumber', null).nativeElement as HTMLElement;
    expect(loading.querySelector('.twin')).not.toBeNull();
    expect(loading.querySelector('.hero')).toBeNull();
  });

  it('shows the health hero from four columns and the compact ring below', () => {
    env();
    expect((body('health', given('health', 'cpuTotal', null, 4)).nativeElement as HTMLElement).querySelector('dy-health-hero')).not.toBeNull();
    expect((body('health', given('health', 'cpuTotal', null, 3)).nativeElement as HTMLElement).querySelector('dy-health-hero')).toBeNull();
  });

  it('draws every kind from live data without errors', () => {
    env();
    for (const [kind, metric] of [['bigNumber', 'cpuTotal'], ['graph', 'networkDown'], ['bars', 'memory'], ['ring', 'memory'], ['heatmap', 'cpuPerCore'], ['topProcesses', 'memory'], ['issues', 'cpuTotal'], ['health', 'cpuTotal'], ['bigNumber', 'fan'], ['graph', 'fan'], ['cpu', 'cpuTotal'], ['diagnosis', 'cpuTotal'], ['events', 'cpuTotal'], ['history', 'cpuTotal'], ['consumers', 'cpuTotal'], ['diskTrend', 'diskFree']] as const) {
      const f = body(kind, given(kind, metric, rd(10)));
      expect((f.nativeElement as HTMLElement).getAttribute('data-kind')).toBe(kind);
      expect((f.nativeElement as HTMLElement).children.length).toBeGreaterThan(0);
    }
  });
});

describe('dy-widget with a performance counter', () => {
  it('shows the counter value, its history and, when it is gone, the reason', async () => {
    env();
    const counters = TestBed.inject(CountersStore);
    const path = '\\System\\Load Average (1 minute)';
    const f = body('graph', given('graph', { perfCounter: path }));
    expect((f.nativeElement as HTMLElement).textContent).toContain('Waiting for the first reading');
    counters.track([path]);
    await counters.tick();
    await counters.tick();
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('.hero b')!.textContent).toMatch(/\d/);
    expect(el.querySelector('dy-sparkline')).not.toBeNull();

    const gone = body('bigNumber', given('bigNumber', { perfCounter: '\\Nothing\\here' }));
    counters.track(['\\Nothing\\here']);
    await counters.tick();
    gone.detectChanges();
    expect((gone.nativeElement as HTMLElement).textContent).toMatch(/not available/);
  });
});

describe('AnimatedNumber', () => {
  // The glide runs on animation frames and the clock: both are faked, so the test advances time instead of waiting for it.
  beforeEach(() => vi.useFakeTimers({ toFake: ['requestAnimationFrame', 'cancelAnimationFrame', 'performance'] }));
  afterEach(() => vi.useRealTimers());

  it('sets text at once for non-numbers and the first value, and glides to the next', () => {
    const f = TestBed.createComponent(AnimatedNumber);
    f.componentRef.setInput('value', '10');
    f.detectChanges();
    expect(f.nativeElement.textContent).toBe('10');
    f.componentRef.setInput('value', 'Normal');
    f.detectChanges();
    expect(f.nativeElement.textContent).toBe('Normal');
    f.componentRef.setInput('value', '20');
    f.detectChanges();
    f.componentRef.setInput('value', '30.5');
    f.detectChanges();
    vi.advanceTimersByTime(150);
    f.detectChanges();
    const mid = Number(f.nativeElement.textContent);
    expect(mid).toBeGreaterThan(20);
    expect(mid).toBeLessThan(30.5);
    vi.advanceTimersByTime(400);
    f.detectChanges();
    expect(f.nativeElement.textContent).toBe('30.5');
  });
});

describe('dy-widget-board', () => {
  it('draws the engine layout on the shared grid, one template per kind, and applies an edit through the engine', async () => {
    env();
    const f = TestBed.createComponent(WidgetBoard);
    const el = f.nativeElement as HTMLElement;
    await vi.waitFor(() => {
      f.detectChanges();
      expect(el.querySelectorAll('pd-widget-frame')).toHaveLength(15);
    });
    expect(el.querySelectorAll('dy-widget')).toHaveLength(15);
    expect(el.querySelector('.pd-wg-edit')?.textContent).toContain('Edit layout');
    expect(el.textContent).not.toContain('Customize dashboard');
    const layouts = TestBed.inject(WidgetLayouts);
    expect(await layouts.edit('dashboard', { remove: { id: 'health' } })).toBeNull();
    await vi.waitFor(() => {
      f.detectChanges();
      expect(el.querySelectorAll('pd-widget-frame')).toHaveLength(14);
    });
    expect(layouts.dashboard()!.canUndo).toBe(true);
    expect(await layouts.edit('dashboard', 'undo')).toBeNull();
    await vi.waitFor(() => {
      f.detectChanges();
      expect(el.querySelectorAll('pd-widget-frame')).toHaveLength(15);
    });
  });

  it('has a template for every dashboard kind the engine offers', async () => {
    env();
    const view = await TestBed.inject(BACKEND).widgetLayout('dashboard');
    const kinds = ('dashboard' in view ? view.dashboard.catalogue.kinds : []).map((k) => k.kind).sort();
    expect([...DASHBOARD_KINDS].sort()).toEqual(kinds);
  });

  it('opens the shared chooser in its add mode in a side sheet and adds through the engine', async () => {
    env();
    const f = TestBed.createComponent(WidgetBoard);
    const el = f.nativeElement as HTMLElement;
    await vi.waitFor(() => {
      f.detectChanges();
      expect(el.querySelectorAll('pd-widget-frame')).toHaveLength(15);
    });
    // The test document has no layout, so the grid cannot edit here (its own button is disabled); "Add widget" is its output.
    f.debugElement.query(By.directive(PdWidgetGrid)).componentInstance.addWidget.emit();
    await vi.waitFor(() => {
      f.detectChanges();
      expect(el.querySelector('aside[role="dialog"] pd-widget-chooser')).not.toBeNull();
    });
    expect(el.querySelector('aside')!.textContent).not.toContain('Shown in the');
    const add = [...el.querySelectorAll<HTMLButtonElement>('aside button')].find((b) => /^Add/.test(b.textContent?.trim() ?? ''));
    expect(add).toBeDefined();
    add!.click();
    await vi.waitFor(() => {
      f.detectChanges();
      expect(el.querySelectorAll('pd-widget-frame')).toHaveLength(16);
    });
    el.querySelector<HTMLButtonElement>('aside .close')!.click();
    f.detectChanges();
    expect(el.querySelector('aside')).toBeNull();
  });
});

describe('dy-widget-board loads', () => {
  it('asks for the history once and shows its failure instead of asking again and again', async () => {
    const mock = env();
    const history = vi.spyOn(mock, 'history').mockRejectedValue({ code: 'internal', message: 'The mock was asked to fail “history”.' });
    const f = TestBed.createComponent(WidgetBoard);
    const el = f.nativeElement as HTMLElement;
    await vi.waitFor(() => {
      f.detectChanges();
      expect(el.textContent).toContain('asked to fail');
    });
    expect(history).toHaveBeenCalledTimes(1);
  });
});
