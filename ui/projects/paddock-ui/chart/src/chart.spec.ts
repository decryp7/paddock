import { ChangeDetectionStrategy, Component, provideZonelessChangeDetection, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { FakeECharts } from '@decryp7/paddock-ui/testing';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { PD_ECHARTS, PdChart, fakeEchartsLoader, reducedMotion } from './chart';
import { donutNarrowHeight } from './options';
import type { PdChartSpec } from './options';
import { assertStatusGlyphs } from '@decryp7/paddock-ui/testing';

/** An ECharts option tree, inspected structurally by these specs. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any -- the option tree is untyped JSON the specs walk by key
type LooseOption = any;

const SPEC: PdChartSpec = { kind: 'bar', x: ['A', 'B'], series: [{ name: 'Jobs', values: [2, 5] }], unit: 'jobs' };

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdChart],
  template: `<pd-chart label="Jobs per day" [spec]="spec()" [option]="option()" [summary]="summary()" [height]="180" [showTitle]="title()" [state]="state()" errorText="Could not reach the engine." (retry)="retried = retried + 1" (chartClick)="clicks.push($event)" />`,
})
class Host {
  readonly spec = signal<PdChartSpec | null>(SPEC);
  readonly option = signal<Record<string, unknown> | null>(null);
  readonly summary = signal<string | null>(null);
  readonly title = signal(true);
  readonly state = signal<'ready' | 'loading' | 'error'>('ready');
  retried = 0;
  readonly clicks: unknown[] = [];
}

async function settle(f: { whenStable(): Promise<unknown> }): Promise<void> {
  await f.whenStable();
  TestBed.tick();
  await f.whenStable();
  await Promise.resolve();
  TestBed.tick();
}

describe('pd-chart', () => {
  afterEach(() => document.documentElement.removeAttribute('data-motion'));

  async function setup() {
    const fake = new FakeECharts();
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), { provide: PD_ECHARTS, useValue: fakeEchartsLoader(fake.init) }] });
    const f = TestBed.createComponent(Host);
    document.body.appendChild(f.nativeElement);
    await settle(f);
    await settle(f);
    return { f, fake };
  }

  it('is a named image with a text twin and the asked height', async () => {
    const { f } = await setup();
    const img = f.nativeElement.querySelector('[role=img]')!;
    expect(img.getAttribute('aria-label')).toBe('Jobs per day');
    const twin = f.nativeElement.querySelector(`#${img.getAttribute('aria-describedby')}`);
    expect(twin?.textContent).toBe('Jobs: 2 jobs to 5 jobs, latest 5 jobs');
    expect(f.nativeElement.querySelector('pd-chart').style.height).toBe('180px');
    f.componentInstance.summary.set('Five jobs on B.');
    await settle(f);
    expect(twin?.textContent).toBe('Five jobs on B.');
  });

  it('draws the label and the unit as the visible title, unless the card around it has one', async () => {
    const { f } = await setup();
    expect(f.nativeElement.querySelector('[data-testid=chart-title]')?.textContent).toBe('Jobs per day (jobs)');
    f.componentInstance.spec.set({ ...SPEC, unit: undefined });
    await settle(f);
    expect(f.nativeElement.querySelector('[data-testid=chart-title]')?.textContent).toBe('Jobs per day');
    f.componentInstance.title.set(false);
    await settle(f);
    expect(f.nativeElement.querySelector('[data-testid=chart-title]')).toBeNull();
  });

  it('puts a donut’s list under the ring and makes the card taller when the box is narrower than 400 px', async () => {
    const resizes: ((e: unknown[]) => void)[] = [];
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(cb: (e: unknown[]) => void) {
          resizes.push(cb);
        }
        observe(): void {}
        unobserve(): void {}
        disconnect(): void {}
      },
    );
    const { f, fake } = await setup();
    f.componentInstance.spec.set({ kind: 'donut', x: ['A', 'B', 'C', 'D'], series: [{ name: 'S', values: [4, 3, 2, 1] }], unit: 'GB' });
    await settle(f);
    const wide = fake.instances[0]!.lastOption as LooseOption;
    expect(wide['legend'].top).toBe('middle');
    resizes.forEach((r) => r([{ contentRect: { width: 320 } }]));
    await settle(f);
    const narrow = fake.instances[0]!.lastOption as LooseOption;
    expect(narrow['legend']).toMatchObject({ left: 'center', bottom: 4 });
    // The ring and the total share one centre, and the ring sits inside the card.
    expect(narrow['series'][0].center).toEqual([narrow['graphic'][0].x, narrow['graphic'][0].y]);
    expect(narrow['series'][0].center[0]).toBe(160);
    expect(f.nativeElement.querySelector('pd-chart').style.height).toBe(`${30 + donutNarrowHeight(4)}px`);
    vi.unstubAllGlobals();
  });

  it('shows ghosted axes while loading and the error with Retry, and measures the chart again when it comes back', async () => {
    const { f, fake } = await setup();
    f.componentInstance.state.set('loading');
    await settle(f);
    expect(f.nativeElement.querySelector('[data-testid=chart-loading]')).not.toBeNull();
    expect(f.nativeElement.querySelector('.pd-surface').hidden).toBe(true);
    f.componentInstance.state.set('error');
    await settle(f);
    expect(f.nativeElement.querySelector('[data-testid=chart-error]')?.textContent).toContain('Could not reach the engine.');
    assertStatusGlyphs(f.nativeElement.querySelector('[data-testid=chart-error]'));
    f.nativeElement.querySelector('.pd-retry').click();
    expect(f.componentInstance.retried).toBe(1);
    const before = fake.instances[0]!.resized;
    f.componentInstance.state.set('ready');
    await settle(f);
    expect(f.nativeElement.querySelector('.pd-surface').hidden).toBe(false);
    expect(fake.instances[0]!.resized).toBe(before + 1);
  });

  it('creates the chart once ECharts is loaded and draws the spec as an option', async () => {
    const { f, fake } = await setup();
    expect(fake.instances).toHaveLength(1);
    const option = fake.instances[0]!.lastOption as LooseOption;
    expect(option['series'][0]).toMatchObject({ type: 'bar', data: [2, 5] });
    f.componentInstance.spec.set({ ...SPEC, series: [{ name: 'Jobs', values: [9, 1] }] });
    await settle(f);
    expect((fake.instances[0]!.lastOption as LooseOption)['series'][0].data).toEqual([9, 1]);
  });

  it('prefers a ready option over the spec', async () => {
    const { f, fake } = await setup();
    f.componentInstance.option.set({ series: [{ type: 'scatter', data: [] }] });
    await settle(f);
    expect(fake.instances[0]!.lastOption).toEqual({ series: [{ type: 'scatter', data: [] }] });
  });

  it('passes a click to the app', async () => {
    const { f, fake } = await setup();
    (fake.instances[0]!.handlers['click'] as (e: unknown) => void)({ name: 'B' });
    expect(f.componentInstance.clicks).toEqual([{ name: 'B' }]);
  });

  it('is created again with new colours when the theme changes, and the option follows', async () => {
    const { f, fake } = await setup();
    document.documentElement.setAttribute('data-motion', 'reduce');
    await new Promise((r) => setTimeout(r, 0));
    await settle(f);
    expect(fake.instances.length).toBeGreaterThan(1);
    expect(fake.instances[0]!.disposed).toBe(true);
    const latest = fake.instances.at(-1)!;
    expect((latest.lastOption as LooseOption)['animation']).toBe(false);
  });

  it('resizes with its box and disposes with the component', async () => {
    const resizes: (() => void)[] = [];
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(cb: () => void) {
          resizes.push(cb);
        }
        observe(): void {}
        unobserve(): void {}
        disconnect(): void {}
      },
    );
    const { f, fake } = await setup();
    resizes.forEach((r) => (r as unknown as (e: unknown[]) => void)([]));
    expect(fake.instances[0]!.resized).toBe(1);
    f.destroy();
    expect(fake.instances[0]!.disposed).toBe(true);
    vi.unstubAllGlobals();
  });

  it('never creates a chart when destroyed before ECharts arrived', async () => {
    const fake = new FakeECharts();
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    TestBed.configureTestingModule({
      providers: [provideZonelessChangeDetection(), { provide: PD_ECHARTS, useValue: async () => (await gate, await fakeEchartsLoader(fake.init)()) }],
    });
    const f = TestBed.createComponent(Host);
    document.body.appendChild(f.nativeElement);
    await settle(f);
    f.destroy();
    release();
    await Promise.resolve();
    await Promise.resolve();
    expect(fake.instances).toHaveLength(0);
  });

  it('knows reduced motion from the app attribute, else from the system', () => {
    document.documentElement.setAttribute('data-motion', 'reduce');
    expect(reducedMotion()).toBe(true);
    document.documentElement.setAttribute('data-motion', 'full');
    expect(reducedMotion()).toBe(false);
    document.documentElement.removeAttribute('data-motion');
    vi.stubGlobal('matchMedia', () => ({ matches: true }));
    expect(reducedMotion()).toBe(true);
    vi.unstubAllGlobals();
  });
});
