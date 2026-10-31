import { provideRouter } from '@angular/router';
import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import type { DashboardWidget, Placement } from '../../core/generated/widgets';
import { WidgetLayouts } from '../../state/widget-layouts';
import { CountersStore } from '../../state/counters.store';
import { LiveStore } from '../../state/live.store';
import { AnimatedNumber } from './animated-number';
import { WidgetBoard } from './widget-board';
import { WidgetView } from './widget-view';

/** A dashboard placement of `kind` showing `metric`. */
function place(kind: DashboardWidget['kind'], metric: DashboardWidget['metric'], span = 3): Placement<DashboardWidget> {
  return { id: `${kind}-1`, span, widget: { kind, metric, options: { rangeSecs: 300, showLabel: true, fill: true, accent: null }, thresholds: { warning: null, critical: null } } };
}

function env() {
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }, provideRouter([])] });
  TestBed.inject(LiveStore).receive(mock.liveUpdate('summary'));
  return mock;
}

describe('dy-widget', () => {
  it('colours a big number by the engine tone only, with the number still printed', () => {
    env();
    const f = TestBed.createComponent(WidgetView);
    f.componentRef.setInput('placement', place('bigNumber', 'cpuTotal'));
    f.componentRef.setInput('metricReading', { value: 95, text: '95', unit: '%', percent: 95, tone: 'bad', reason: null });
    f.detectChanges();
    const hero = (f.nativeElement as HTMLElement).querySelector('.hero')!;
    expect(hero.getAttribute('data-tone')).toBe('bad');
    expect(hero.textContent).toContain('95');
    f.componentRef.setInput('metricReading', { value: 5, text: '5', unit: '%', percent: 5, tone: 'ok', reason: null });
    f.detectChanges();
    expect(hero.getAttribute('data-tone')).toBe('ok');
  });

  it('says why a graph has no reading instead of drawing a history for it', () => {
    env();
    const f = TestBed.createComponent(WidgetView);
    f.componentRef.setInput('placement', place('graph', 'diskActivity'));
    f.componentRef.setInput('metricReading', { value: null, text: '—', unit: '', percent: null, tone: 'ok', reason: "The disk's busy time is not reported here." });
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('dy-sparkline')).toBeNull();
    expect(el.querySelector('.note')?.textContent).toContain('not reported here');
    f.componentRef.setInput('metricReading', { value: 40, text: '40', unit: '%', percent: 40, tone: 'ok', reason: null });
    f.detectChanges();
    expect(el.querySelector('dy-sparkline')).not.toBeNull();
    expect(el.querySelector('.note')).toBeNull();
  });

  it('draws every kind from live data without errors', () => {
    env();
    for (const [kind, metric] of [['bigNumber', 'cpuTotal'], ['graph', 'networkDown'], ['bars', 'memory'], ['ring', 'memory'], ['heatmap', 'cpuPerCore'], ['topProcesses', 'memory'], ['issues', 'cpuTotal'], ['health', 'cpuTotal'], ['bigNumber', 'fan'], ['graph', 'fan'], ['cpu', 'cpuTotal'], ['diagnosis', 'cpuTotal'], ['events', 'cpuTotal'], ['history', 'cpuTotal'], ['consumers', 'cpuTotal'], ['diskTrend', 'diskFree']] as const) {
      const f = TestBed.createComponent(WidgetView);
      f.componentRef.setInput('placement', place(kind, metric));
      f.detectChanges();
      expect((f.nativeElement as HTMLElement).getAttribute('data-kind')).toBe(kind);
      expect((f.nativeElement as HTMLElement).textContent!.length).toBeGreaterThan(0);
    }
  });
});

describe('dy-widget with a performance counter', () => {
  it('shows the counter value, its history and, when it is gone, the reason', async () => {
    env();
    const counters = TestBed.inject(CountersStore);
    const path = '\\System\\Load Average (1 minute)';
    const f = TestBed.createComponent(WidgetView);
    f.componentRef.setInput('placement', place('graph', { perfCounter: path }));
    f.detectChanges();
    expect((f.nativeElement as HTMLElement).textContent).toContain('Waiting for the first reading');
    counters.track([path]);
    await counters.tick();
    await counters.tick();
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('h3')!.textContent).toBe('Load Average (1 minute)');
    expect(el.querySelector('.hero b')!.textContent).toMatch(/\d/);
    expect(el.querySelector('dy-sparkline')).not.toBeNull();

    const gone = TestBed.createComponent(WidgetView);
    gone.componentRef.setInput('placement', place('bigNumber', { perfCounter: '\\Nothing\\here' }));
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
  it('draws the engine layout, opens the shared chooser and applies an edit through the engine', async () => {
    env();
    const f = TestBed.createComponent(WidgetBoard);
    const el = f.nativeElement as HTMLElement;
    await vi.waitFor(() => {
      f.detectChanges();
      expect(el.querySelectorAll('dy-widget')).toHaveLength(15);
    });
    [...el.querySelectorAll('button')].find((b) => b.textContent?.includes('Customize dashboard'))!.click();
    f.detectChanges();
    expect(el.querySelector('pd-widget-chooser')).not.toBeNull();
    const layouts = TestBed.inject(WidgetLayouts);
    expect(await layouts.edit('dashboard', { remove: { id: 'health' } })).toBeNull();
    await vi.waitFor(() => {
      f.detectChanges();
      expect(el.querySelectorAll('dy-widget')).toHaveLength(14);
    });
    expect(layouts.dashboard()!.canUndo).toBe(true);
    expect(await layouts.edit('dashboard', 'undo')).toBeNull();
    await vi.waitFor(() => {
      f.detectChanges();
      expect(el.querySelectorAll('dy-widget')).toHaveLength(15);
    });
  });
});
