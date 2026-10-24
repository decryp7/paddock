import { Component, provideZonelessChangeDetection, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { FakeECharts } from '@decryp7/paddock-ui/testing';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { PD_ECHARTS, PdChart, fakeEchartsLoader, reducedMotion } from './chart';
import type { PdChartSpec } from './options';

const SPEC: PdChartSpec = { kind: 'bar', x: ['A', 'B'], series: [{ name: 'Jobs', values: [2, 5] }], unit: 'jobs' };

@Component({
  imports: [PdChart],
  template: `<pd-chart label="Jobs per day" [spec]="spec()" [option]="option()" [summary]="summary()" [height]="180" (chartClick)="clicks.push($event)" />`,
})
class Host {
  readonly spec = signal<PdChartSpec | null>(SPEC);
  readonly option = signal<Record<string, unknown> | null>(null);
  readonly summary = signal<string | null>(null);
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

  it('creates the chart once ECharts is loaded and draws the spec as an option', async () => {
    const { f, fake } = await setup();
    expect(fake.instances).toHaveLength(1);
    const option = fake.instances[0]!.lastOption as Record<string, any>;
    expect(option['series'][0]).toMatchObject({ type: 'bar', data: [2, 5] });
    f.componentInstance.spec.set({ ...SPEC, series: [{ name: 'Jobs', values: [9, 1] }] });
    await settle(f);
    expect((fake.instances[0]!.lastOption as Record<string, any>)['series'][0].data).toEqual([9, 1]);
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
    expect((latest.lastOption as Record<string, any>)['animation']).toBe(false);
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
    resizes.forEach((r) => r());
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
