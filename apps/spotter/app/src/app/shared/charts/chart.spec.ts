import { Component, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { ChartTokens } from '../../core/chart-tokens';
import { fakeChart, flush } from '../../../testing/test-helpers';
import { CHART_LOADER, Chart, type ChartLoader } from './chart';

@Component({ imports: [Chart], template: `@if (show()) {<sp-chart [option]="option()" label="Releases per day" />}` })
class Host {
  readonly option = signal<unknown>({ a: 1 });
  readonly show = signal(true);
}

describe('Chart', () => {
  const originalRO = globalThis.ResizeObserver;
  afterEach(() => {
    globalThis.ResizeObserver = originalRO;
  });

  function setup(loader: ChartLoader) {
    TestBed.configureTestingModule({ providers: [{ provide: CHART_LOADER, useValue: loader }] });
    const fixture = TestBed.createComponent(Host);
    fixture.detectChanges();
    return fixture;
  }

  it('labels the chart, draws the option and redraws on change', async () => {
    const chart = fakeChart();
    const f = setup(() => Promise.resolve(() => chart));
    await flush();
    expect(f.nativeElement.querySelector('[role=img]').getAttribute('aria-label')).toBe('Releases per day');
    expect(chart.options).toEqual([{ a: 1 }]);
    f.componentInstance.option.set({ a: 2 });
    f.detectChanges();
    await flush();
    expect(chart.options).toEqual([{ a: 1 }, { a: 2 }]);
  });

  it('rebuilds the chart when the colour scheme changes', async () => {
    const charts: ReturnType<typeof fakeChart>[] = [];
    const f = setup(() => Promise.resolve(() => charts[charts.push(fakeChart()) - 1]!));
    await flush();
    // The shell's appearance store flips this attribute; the MutationObserver reports it asynchronously.
    document.documentElement.setAttribute('data-theme', 'dark');
    try {
      await flush();
      f.detectChanges();
      await flush();
    } finally {
      document.documentElement.removeAttribute('data-theme');
    }
    expect(charts).toHaveLength(2);
    expect(charts[0]!.disposed).toBe(true);
    expect(charts[1]!.options).toHaveLength(1);
  });

  it('stays empty when the library cannot load', async () => {
    const f = setup(() => Promise.reject(new Error('chunk failed')));
    await flush();
    expect(f.nativeElement.querySelector('[role=img]')).not.toBeNull();
  });

  it('resizes with its container and cleans up on destroy', async () => {
    const callbacks: (() => void)[] = [];
    const disconnect = vi.fn();
    globalThis.ResizeObserver = class {
      constructor(cb: () => void) {
        callbacks.push(cb);
      }
      observe = vi.fn();
      disconnect = disconnect;
      unobserve = vi.fn();
    } as unknown as typeof ResizeObserver;
    const chart = fakeChart();
    const resize = vi.spyOn(chart, 'resize');
    const f = setup(() => Promise.resolve(() => chart));
    await flush();
    callbacks[0]!();
    expect(resize).toHaveBeenCalled();
    f.componentInstance.show.set(false);
    f.detectChanges();
    expect(chart.disposed).toBe(true);
    expect(disconnect).toHaveBeenCalled();
  });

  it('does not draw when destroyed while the library was loading', async () => {
    let finish!: () => void;
    const chart = fakeChart();
    const f = setup(() => new Promise((r) => (finish = () => r(() => chart))));
    f.componentInstance.show.set(false);
    f.detectChanges();
    finish();
    await flush();
    expect(chart.options).toEqual([]);
  });

  it('works without ResizeObserver', async () => {
    (globalThis as { ResizeObserver?: unknown }).ResizeObserver = undefined;
    const chart = fakeChart();
    setup(() => Promise.resolve(() => chart));
    await flush();
    expect(chart.options).toHaveLength(1);
  });
});

describe('the real chart loader', () => {
  it('loads ECharts lazily and returns a factory without drawing anything', async () => {
    const loader = TestBed.inject(CHART_LOADER);
    const factory = await loader(TestBed.inject(ChartTokens).tokens());
    expect(typeof factory).toBe('function');
  });
});
