import { provideRouter } from '@angular/router';
import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import type { DashboardWidget, Placement } from '../../core/generated/widgets';
import { newDashboardWidget } from './new-widget';
import { WidgetLayouts } from '../../state/widget-layouts';
import { CountersStore } from '../../state/counters.store';
import { LiveStore } from '../../state/live.store';
import { AnimatedNumber } from './animated-number';
import { WidgetBoard } from './widget-board';
import { WidgetView } from './widget-view';

/** A dashboard placement of `kind` showing `metric`. */
function place(kind: DashboardWidget['kind'], metric: DashboardWidget['metric'], span = 3): Placement<DashboardWidget> {
  return { id: `${kind}-1`, span, widget: { ...newDashboardWidget(kind), metric } };
}

function env() {
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }, provideRouter([])] });
  TestBed.inject(LiveStore).receive(mock.liveUpdate('summary'));
  return mock;
}

describe('dy-widget', () => {
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
  it('sets text at once for non-numbers and the first value, and glides to the next', async () => {
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
    await vi.waitFor(() => {
      f.detectChanges();
      expect(f.nativeElement.textContent).toBe('30.5');
    }, { timeout: 5_000 }); // the glide is about 300 ms; many times that on a loaded machine
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
