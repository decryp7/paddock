import { Component, signal } from '@angular/core';
import { TestBed, type ComponentFixture } from '@angular/core/testing';
import type { EChartsCoreOption } from 'echarts/core';
import { CHART_FACTORY, EChart, type ChartFactory } from './echart';
import { FakeChart } from '../../../testing/render';

@Component({
  imports: [EChart],
  template: `<pw-echart [option]="option()" [themeKey]="theme()" [height]="'100px'" label="A test chart" (chartEvent)="events.push($event)" />`,
})
class Host {
  readonly option = signal<EChartsCoreOption>({ series: [{ data: [1] }] });
  readonly theme = signal('dark:1h');
  readonly events: { type: string; params: unknown }[] = [];
}

let frames: FrameRequestCallback[] = [];
let resizeCallbacks: (() => void)[] = [];
let disconnects = 0;

const flushFrames = () => {
  const run = frames;
  frames = [];
  run.forEach((f) => f(0));
};
const tick = () => new Promise((r) => setTimeout(r, 0));

async function setup(factory?: ChartFactory) {
  FakeChart.all = [];
  TestBed.configureTestingModule({ providers: [{ provide: CHART_FACTORY, useValue: factory ?? (() => Promise.resolve(new FakeChart())) }] });
  const fixture: ComponentFixture<Host> = TestBed.createComponent(Host);
  fixture.detectChanges();
  await tick();
  fixture.detectChanges();
  return { fixture, host: fixture.componentInstance, el: fixture.nativeElement.querySelector('pw-echart') as HTMLElement };
}

describe('EChart', () => {
  beforeEach(() => {
    frames = [];
    resizeCallbacks = [];
    disconnects = 0;
    vi.stubGlobal('requestAnimationFrame', (f: FrameRequestCallback) => frames.push(f));
    vi.stubGlobal('cancelAnimationFrame', vi.fn());
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(cb: () => void) {
          resizeCallbacks.push(cb);
        }
        observe() {}
        disconnect() {
          disconnects++;
        }
      },
    );
  });
  afterEach(() => vi.unstubAllGlobals());

  it('is an accessible image with the label and height', async () => {
    const { el } = await setup();
    expect(el.getAttribute('role')).toBe('img');
    expect(el.getAttribute('aria-label')).toBe('A test chart');
    expect(el.style.height).toBe('100px');
  });

  it('applies the first option as a full replace once the chart exists', async () => {
    const { host } = await setup();
    const chart = FakeChart.all[0]!;
    expect(chart.options.length).toBe(1);
    expect(chart.options[0]!.option).toBe(host.option());
    expect(chart.options[0]!.opts).toEqual({ notMerge: true, lazyUpdate: false });
  });

  it('coalesces updates into one merging setOption per frame', async () => {
    const { fixture, host } = await setup();
    const chart = FakeChart.all[0]!;
    const a: EChartsCoreOption = { series: [{ data: [2] }] };
    const b: EChartsCoreOption = { series: [{ data: [3] }] };
    host.option.set(a);
    fixture.detectChanges();
    host.option.set(b);
    fixture.detectChanges();
    flushFrames();
    flushFrames();
    expect(chart.options.length).toBe(2);
    expect(chart.options[1]!.option).toBe(b);
    expect(chart.options[1]!.opts).toEqual({ notMerge: false, lazyUpdate: true });
  });

  it('replaces the option again when the theme key changes', async () => {
    const { fixture, host } = await setup();
    const chart = FakeChart.all[0]!;
    host.theme.set('light:1h');
    fixture.detectChanges();
    flushFrames();
    expect(chart.options.at(-1)!.opts).toEqual({ notMerge: true, lazyUpdate: false });
    host.option.set({ series: [] });
    fixture.detectChanges();
    flushFrames();
    expect(chart.options.at(-1)!.opts).toEqual({ notMerge: false, lazyUpdate: true });
  });

  it('keeps only the latest option while the chart is still loading', async () => {
    let resolve!: (c: FakeChart) => void;
    const { fixture, host } = await setup(() => new Promise<FakeChart>((r) => (resolve = r)));
    host.option.set({ series: [{ data: [9] }] });
    fixture.detectChanges();
    flushFrames();
    const chart = new FakeChart();
    resolve(chart);
    await tick();
    expect(chart.options.length).toBe(1);
    expect(chart.options[0]!.option).toEqual({ series: [{ data: [9] }] });
  });

  it('emits zoom and click events from the chart', async () => {
    const { host } = await setup();
    const chart = FakeChart.all[0]!;
    chart.handlers.get('datazoom')!({ start: 10 });
    chart.handlers.get('click')!({ seriesName: 'RAM' });
    expect(host.events).toEqual([
      { type: 'datazoom', params: { start: 10 } },
      { type: 'click', params: { seriesName: 'RAM' } },
    ]);
  });

  it('resizes with its container, and not before the chart exists', async () => {
    await setup();
    const chart = FakeChart.all[0]!;
    resizeCallbacks[0]!();
    expect(chart.resized).toBe(1);
  });

  it('ignores a resize before the chart is created', async () => {
    const { fixture } = await setup(() => new Promise<FakeChart>(() => undefined));
    expect(() => resizeCallbacks[0]!()).not.toThrow();
    fixture.destroy();
  });

  it('disposes the chart, stops observing and cancels a pending frame on destroy', async () => {
    const { fixture, host } = await setup();
    const chart = FakeChart.all[0]!;
    host.option.set({ series: [] });
    fixture.detectChanges();
    fixture.destroy();
    expect(chart.disposed).toBe(true);
    expect(disconnects).toBe(1);
    expect(cancelAnimationFrame).toHaveBeenCalled();
  });

  it('disposes a chart that finishes loading after the component was destroyed', async () => {
    let resolve!: (c: FakeChart) => void;
    const { fixture } = await setup(() => new Promise<FakeChart>((r) => (resolve = r)));
    fixture.destroy();
    const late = new FakeChart();
    resolve(late);
    await tick();
    expect(late.disposed).toBe(true);
    expect(late.options.length).toBe(0);
  });
});
