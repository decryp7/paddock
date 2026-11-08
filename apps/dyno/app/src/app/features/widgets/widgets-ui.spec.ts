import { provideRouter } from '@angular/router';
import { TestBed } from '@angular/core/testing';
import { By } from '@angular/platform-browser';
import { PD_ECHARTS, fakeEchartsLoader } from '@decryp7/paddock-ui/chart';
import { FakeECharts } from '@decryp7/paddock-ui/testing';
import { PdWidgetGrid } from '@decryp7/paddock-ui/widgets';
import { BACKEND } from '../../core/backend';
import { MockBackend, type MockOptions } from '../../core/mock-backend';
import { mockWidgetChoices } from '../../core/mock/widgets';
import type { Cells, DashboardWidget, MetricReading, Placement } from '../../core/generated/widgets';
import { WidgetLayouts } from '../../state/widget-layouts';
import { DiagnosisStore } from '../../state/diagnosis.store';
import { CountersStore } from '../../state/counters.store';
import { ActivityStore } from '../../state/activity.store';
import { AlertsStore } from '../../state/alerts.store';
import { LiveStore } from '../../state/live.store';
import { AnimatedNumber, GLIDE_STEPS, glideSteps } from './animated-number';
import { DASHBOARD_KINDS, WidgetBoard } from './widget-board';
import { CdkVirtualScrollViewport } from '@angular/cdk/scrolling';
import { EVENTS_LOADED } from '../dashboard/events-feed';
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

function env(options: MockOptions = {}) {
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000, ...options });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }, provideRouter([]), { provide: PD_ECHARTS, useValue: fakeEchartsLoader(new FakeECharts().init) }] });
  TestBed.inject(LiveStore).receive(mock.liveUpdate('summary'));
  return mock;
}

const rd = (value: number | null, tone: MetricReading['tone'] = 'ok', extra: Partial<MetricReading> = {}): MetricReading => ({ value, text: value === null ? '—' : String(value), unit: '%', percent: value, tone, reason: null, ...extra });

describe('dy-widget', () => {
  it('names who read the temperature: the helper, or the reason nobody could on this Mac', () => {
    const mock = env();
    const update = mock.liveUpdate('summary');
    TestBed.inject(LiveStore).receive({ ...update, via: { ...update.via, temperatures: 'helper' } });
    const el = body('bigNumber', given('bigNumber', 'temperature', rd(61))).nativeElement as HTMLElement;
    expect(el.querySelector('[data-testid="reading-source-helper"]')).not.toBeNull();
    const why = 'macOS reports a thermal pressure level, not temperatures, to software.';
    TestBed.inject(LiveStore).receive({ ...update, via: { ...update.via, temperatures: { unavailable: { reason: why, next: null } } } });
    const g = body('bigNumber', given('bigNumber', 'temperature', rd(null, 'ok', { reason: why }))).nativeElement as HTMLElement;
    expect(g.querySelector('[data-testid="reading-source-unavailable"]')?.textContent).toContain('thermal pressure');
    const other = body('bigNumber', given('bigNumber', 'cpuTotal', rd(5))).nativeElement as HTMLElement;
    expect(other.querySelector('dy-reading-source')).toBeNull();
  });

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
    expect(el.querySelector('.rn')!.textContent).toContain('%');
  });

  it('says the battery state beside the ring instead of repeating the title, and nothing for another ring', () => {
    const { mock } = (() => { const m = env(); return { mock: m }; })();
    const el = body('ring', given('ring', 'battery', rd(86))).nativeElement as HTMLElement;
    expect(el.querySelector('.state')?.textContent).toMatch(/^(Charging|On battery|Full)$/);
    expect(el.textContent).not.toContain('Battery');
    const live = TestBed.inject(LiveStore);
    const base = mock.liveUpdate('summary');
    const battery = (source: 'ac' | 'battery', charge: number) => ({ ...base, system: { ...base.system, battery: { available: { charge, source, health: 90, cycles: null, secondsToEmpty: null } } } });
    for (const [source, charge, word] of [['ac', 100, 'Full'], ['ac', 80, 'Charging'], ['battery', 80, 'On battery']] as const) {
      live.receive(battery(source, charge));
      const f = body('ring', given('ring', 'battery', rd(charge)));
      expect((f.nativeElement as HTMLElement).querySelector('.state')?.textContent).toBe(word);
    }
    live.receive({ ...base, system: { ...base.system, battery: { unavailable: 'No battery' } } });
    expect((body('ring', given('ring', 'battery', rd(86))).nativeElement as HTMLElement).querySelector('.state')).toBeNull();
    expect((body('ring', given('ring', 'memory', rd(51))).nativeElement as HTMLElement).querySelector('.note')).toBeNull();
  });

  it('does not repeat the title in the compact health ring, only the grade', () => {
    env();
    const el = body('health', given('health', 'cpuTotal', null, 3)).nativeElement as HTMLElement;
    expect(el.textContent).not.toContain('Health score');
  });

  it('lists every issue however short the widget, in a focusable scrolling region, and never ends in "+n more"', async () => {
    env({ longLists: true });
    await TestBed.inject(DiagnosisStore).loadAnomalies();
    const total = TestBed.inject(DiagnosisStore).anomalies().length;
    expect(total).toBeGreaterThanOrEqual(50);
    const at = (h: number): DyValue => ({ placement: { ...place('issues', 'cpuTotal', 3), h }, reading: null });
    for (const h of [4, 20]) {
      const el = body('issues', at(h)).nativeElement as HTMLElement;
      expect(el.querySelectorAll('.issues li').length).toBe(total);
      expect(el.textContent).not.toMatch(/\+\d+ more/);
      const region = el.querySelector('.issues-scroll')!;
      expect(region.getAttribute('tabindex')).toBe('0');
      expect(region.getAttribute('aria-label')).toBe('Unusual things found');
    }
    const el = body('issues', at(4)).nativeElement as HTMLElement;
    expect(el.querySelector('.issues strong')!.classList.contains('two-lines')).toBe(true);
  });

  it('lists the recent events in a virtual viewport holding every entry, never ending in "+n more"', async () => {
    env({ longLists: true });
    await TestBed.inject(ActivityStore).load(200);
    await TestBed.inject(AlertsStore).load(200);
    const feed = TestBed.inject(ActivityStore).entries().length + TestBed.inject(AlertsStore).alerts().length;
    expect(feed).toBeGreaterThanOrEqual(50);
    const at = (h: number): DyValue => ({ placement: { ...place('events', 'cpuTotal', 6), h }, reading: null });
    for (const h of [4, 30]) {
      const f = body('events', at(h));
      const el = f.nativeElement as HTMLElement;
      const viewport = f.debugElement.query((d) => d.componentInstance instanceof CdkVirtualScrollViewport);
      expect((viewport.componentInstance as CdkVirtualScrollViewport).getDataLength()).toBe(feed);
      expect(viewport.nativeElement.getAttribute('aria-label')).toBe('Activity and alerts');
      expect(viewport.nativeElement.getAttribute('tabindex')).toBe('0');
      expect(el.textContent).not.toMatch(/\+\d+ more/);
      expect(el.querySelector('.more')).toBeNull();
    }
  });

  it('says the feed shows only the latest entries when a list is as long as the board asked for, and not before', () => {
    env();
    const at = (n: number) => 1_700_000_000_000 - n * 1000;
    const entries = (n: number) => Array.from({ length: n }, (_, i) => ({ id: `e${i}`, at: at(i), kind: 'system' as const, title: `Note ${i}`, detail: 'Invented.' }));
    const store = TestBed.inject(ActivityStore);
    for (const e of entries(EVENTS_LOADED - 1)) store.receive(e);
    const f = body('events', { placement: { ...place('events', 'cpuTotal', 6), h: 8 }, reading: null });
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('.more')).toBeNull();
    store.receive({ id: 'last', at: at(EVENTS_LOADED), kind: 'system', title: 'One more', detail: 'Invented.' });
    f.detectChanges();
    expect(el.querySelector('.more')!.textContent).toContain(`Showing the latest ${EVENTS_LOADED} events`);
  });

  it('says why a graph has no reading instead of drawing a history for it', () => {
    env();
    const f = body('graph', given('graph', 'diskActivity', rd(null, 'ok', { reason: "The disk's busy time is not reported here." })));
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('pd-sparkline')).toBeNull();
    expect(el.querySelector('.note')?.textContent).toContain('not reported here');
    f.componentRef.setInput('value', given('graph', 'diskActivity', rd(40)));
    f.detectChanges();
    expect(el.querySelector('pd-sparkline')).not.toBeNull();
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
    expect(el.querySelector('pd-sparkline')).not.toBeNull();
    // Wiring: a short history starts well right of the left edge (the box holds SPARK_CAPACITY samples) and has no padding.
    const d = el.querySelector('pd-sparkline .pd-line')!.getAttribute('d')!;
    expect(Number(/^M(\d+(\.\d+)?) /.exec(d)![1])).toBeGreaterThan(100);
    expect(d).toContain('L120.0 ');
    expect(el.querySelector('pd-sparkline .pd-dot')).toBeNull();
    expect(el.querySelector('pd-sparkline svg')!.getAttribute('aria-label')).not.toMatch(/\.\d{3,}/);

    const gone = body('bigNumber', given('bigNumber', { perfCounter: '\\Nothing\\here' }));
    counters.track(['\\Nothing\\here']);
    await counters.tick();
    gone.detectChanges();
    expect((gone.nativeElement as HTMLElement).textContent).toMatch(/not available/);
  });
});

describe('glideSteps', () => {
  it('eases out in a few steps and ends on the exact text', () => {
    const steps = glideSteps(20, 30.5, '30.50', 2);
    expect(steps).toHaveLength(GLIDE_STEPS);
    expect(steps.at(-1)).toBe('30.50');
    const values = [20, ...steps.map(Number)];
    const moves = values.slice(1).map((v, i) => v - (values[i] ?? 0));
    expect(moves.every((m) => m > 0)).toBe(true);
    // Ease-out: each step covers less ground than the one before.
    expect(moves).toEqual([...moves].sort((a, b) => b - a));
    expect(glideSteps(5, 1, '1', 0).at(-1)).toBe('1');
  });
});

describe('AnimatedNumber', () => {
  // The glide runs on the timer: it is faked, so the test advances time instead of waiting for it.
  beforeEach(() => vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] }));
  afterEach(() => vi.useRealTimers());

  // A glide used to repaint on every animation frame (about 18 frames per update, every second on the dashboard); it
  // now shows GLIDE_STEPS values, so a live dashboard repaints a few times per update.
  it('glides in a bounded number of repaints', () => {
    const f = TestBed.createComponent(AnimatedNumber);
    f.componentRef.setInput('value', '10');
    f.detectChanges();
    f.componentRef.setInput('value', '40');
    f.detectChanges();
    const shown: string[] = [];
    for (let t = 0; t < 400; t += 10) {
      vi.advanceTimersByTime(10);
      f.detectChanges();
      const now = f.nativeElement.textContent as string;
      if (shown.at(-1) !== now) shown.push(now);
    }
    expect(shown).toHaveLength(GLIDE_STEPS);
    expect(shown.at(-1)).toBe('40');
    expect(vi.getTimerCount()).toBe(0);
  });

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

  it('gives the grid the page scroller, the shell main element, for auto-scroll while dragging', async () => {
    env();
    const main = document.createElement('main');
    document.body.append(main);
    const f = TestBed.createComponent(WidgetBoard);
    main.append(f.nativeElement);
    await vi.waitFor(() => {
      f.detectChanges();
      expect(f.debugElement.query(By.directive(PdWidgetGrid)).componentInstance.scroller()).toBe(main);
    });
    main.remove();
  });

  it('has a template for every dashboard kind the engine offers', async () => {
    env();
    const view = await TestBed.inject(BACKEND).widgetLayout('dashboard');
    const kinds = ('dashboard' in view ? view.dashboard.catalogue.kinds : []).map((k) => k.kind).sort();
    expect([...DASHBOARD_KINDS].sort()).toEqual(kinds);
  });

  it('offers a metric only for kinds that pick one, never for a panel kind', async () => {
    const mock = env();
    // The engine lets every kind but heatmap, ring and top processes show any metric, so a panel kind arrives with several.
    const choices = mockWidgetChoices();
    const several = choices.dashboard.find((k) => k.kind === 'graph')!.metrics;
    choices.dashboard = choices.dashboard.map((k) => (k.kind === 'cpu' || k.kind === 'ring' ? { ...k, metrics: several } : k));
    vi.spyOn(mock, 'widgetChoices').mockResolvedValue(choices);
    const f = TestBed.createComponent(WidgetBoard);
    const board = f.componentInstance as unknown as { hasOptions(kind: string): boolean; choices: { ready(): boolean } };
    await vi.waitFor(() => expect(board.choices.ready()).toBe(true));
    expect(board.hasOptions('ring')).toBe(true);
    expect(board.hasOptions('cpu')).toBe(false);
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
