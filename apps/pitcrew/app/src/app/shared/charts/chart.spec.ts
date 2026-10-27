import { Component, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { CHART_LOADER, Chart, type ChartInstance } from './chart';

@Component({
  imports: [Chart],
  template: `<pc-chart [option]="option()" label="Jobs per day" [height]="150" />`,
})
class Host {
  readonly option = signal<Record<string, unknown>>({ a: 1 });
}

function fakeInstance() {
  return { setOption: vi.fn<ChartInstance['setOption']>(), resize: vi.fn<() => void>(), dispose: vi.fn<() => void>() };
}

describe('Chart', () => {
  afterEach(() => vi.unstubAllGlobals());

  async function mount(loader: () => Promise<(el: HTMLElement) => ChartInstance>) {
    TestBed.configureTestingModule({ providers: [{ provide: CHART_LOADER, useValue: loader }] });
    const fixture = TestBed.createComponent(Host);
    fixture.detectChanges();
    await fixture.whenStable();
    fixture.detectChanges();
    return fixture;
  }

  it('creates the chart in the host, applies options and disposes on destroy', async () => {
    const chart = fakeInstance();
    const factory = vi.fn((_el: HTMLElement) => chart);
    const fixture = await mount(() => Promise.resolve(factory));
    const host = fixture.nativeElement.querySelector('.host') as HTMLElement;
    expect(host.getAttribute('aria-label')).toBe('Jobs per day');
    expect(host.style.height).toBe('150px');
    expect(factory).toHaveBeenCalledWith(host);
    expect(chart.setOption).toHaveBeenCalledWith({ a: 1 }, { notMerge: true });

    fixture.componentInstance.option.set({ b: 2 });
    fixture.detectChanges();
    await fixture.whenStable();
    expect(chart.setOption).toHaveBeenLastCalledWith({ b: 2 }, { notMerge: true });

    fixture.destroy();
    expect(chart.dispose).toHaveBeenCalledTimes(1);
  });

  it('resizes with its container when ResizeObserver exists', async () => {
    let callback: () => void = () => undefined;
    const disconnect = vi.fn();
    class FakeRO {
      constructor(cb: () => void) {
        callback = cb;
      }
      observe = vi.fn();
      disconnect = disconnect;
    }
    vi.stubGlobal('ResizeObserver', FakeRO);
    const chart = fakeInstance();
    const fixture = await mount(() => Promise.resolve(() => chart));
    callback();
    expect(chart.resize).toHaveBeenCalled();
    fixture.destroy();
    expect(disconnect).toHaveBeenCalled();
  });

  it('shows the failure text when the loader rejects', async () => {
    const fixture = await mount(() => Promise.reject(new Error('no echarts')));
    expect(fixture.nativeElement.textContent).toContain('The chart could not be drawn.');
  });

  it('shows the failure text when the factory throws or setOption throws', async () => {
    const f1 = await mount(() =>
      Promise.resolve(() => {
        throw new Error('bad');
      }),
    );
    expect(f1.nativeElement.textContent).toContain('could not be drawn');
    TestBed.resetTestingModule();
    const chart = fakeInstance();
    chart.setOption.mockImplementation(() => {
      throw new Error('bad option');
    });
    const f2 = await mount(() => Promise.resolve(() => chart));
    expect(f2.nativeElement.textContent).toContain('could not be drawn');
  });

  it('does not create a chart when destroyed before the loader resolved', async () => {
    let resolve!: (f: (el: HTMLElement) => ChartInstance) => void;
    const factory = vi.fn(() => fakeInstance());
    TestBed.configureTestingModule({
      providers: [{ provide: CHART_LOADER, useValue: () => new Promise((r) => (resolve = r)) }],
    });
    const fixture = TestBed.createComponent(Host);
    fixture.detectChanges();
    await Promise.resolve();
    fixture.destroy();
    resolve(factory);
    await Promise.resolve();
    expect(factory).not.toHaveBeenCalled();
  });
});
