import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import { provideRouter } from '@angular/router';
import { patchState, type WritableStateSource } from '@ngrx/signals';
import type { GpuSample, MemorySample, Overview, Throughput } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { LiveStore } from '../../core/live.store';
import { MockBackend } from '../../core/mock-backend';
import { registerIcons } from '../../shared/icons';
import { settle } from '../../../testing/render';

import { StatTiles } from './stat-tiles';

const GiB = 1000 ** 3;
const gpu = (over: Partial<GpuSample> = {}): GpuSample => ({ name: 'NVIDIA GeForce RTX 4070', vramTotal: 12 * GiB, vramUsed: 6 * GiB, utilisation: 40, unified: false, ...over });
const mem = (over: Partial<MemorySample> = {}): MemorySample => ({
  at: 0,
  ramTotal: 32 * GiB,
  ramAvailable: 16 * GiB,
  commitUsed: 20 * GiB,
  commitLimit: 40 * GiB,
  swapUsed: GiB,
  pressure: 'normal',
  cpuPercent: 23.6,
  gpus: [gpu()],
  pitwallRss: 4 * GiB,
  foregroundBusy: false,
  ...over,
});

async function mount(patch?: (o: Overview) => Overview, backend = new MockBackend()) {
  TestBed.configureTestingModule({ imports: [StatTiles], providers: [{ provide: BACKEND, useValue: backend }, provideRouter([])] });
  registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
  const live = TestBed.inject(LiveStore);
  await live.load();
  const set = (p: (o: Overview) => Overview) => patchState(live as unknown as WritableStateSource<object>, { overview: p(live.overview()!) });
  if (patch) set(patch);
  const fixture = TestBed.createComponent(StatTiles);
  await settle(fixture);
  const tile = (i: number) => (fixture.nativeElement as HTMLElement).querySelectorAll('section.tile')[i] as HTMLElement;
  return { fixture, el: fixture.nativeElement as HTMLElement, live, tile, set };
}

/** Text without whitespace: inline elements are glued together in the DOM, so compare compactly. */
const text = (e: Element) => e.textContent!.replace(/\s+/g, '');

describe('StatTiles', () => {
  it('shows skeletons in every tile until the overview arrives', async () => {
    const { el } = await mount(undefined, new MockBackend({ failing: ['overview'] }));
    expect(el.querySelectorAll('section.tile').length).toBe(4);
    expect(el.querySelectorAll('pw-skeleton').length).toBe(4);
  });

  it('shows used system memory, the Pitwall share and the pressure', async () => {
    const { tile } = await mount((o) => ({ ...o, memory: mem() }));
    const t = tile(0);
    expect(text(t.querySelector('.value')!)).toBe('16.0GBof32GB');
    // Normal pressure is the calm state: nothing is said about it.
    expect(t.querySelector('.status')).toBeNull();
    expect(t.querySelector('.stack')!.getAttribute('aria-label')).toBe('50 percent used, 13 percent by Pitwall');
    expect((t.querySelector('.seg.own') as HTMLElement).style.width).toBe('13%');
    expect((t.querySelector('.seg.other') as HTMLElement).style.width).toBe('37%');
    expect(text(t.querySelector('.foot')!)).toBe('Pitwall4.0GB16.0GBfree');
  });

  it('caps the Pitwall share at the used share and shows critical pressure', async () => {
    const { tile } = await mount((o) => ({ ...o, memory: mem({ ramAvailable: 29 * GiB, pitwallRss: 10 * GiB, pressure: 'critical' }) }));
    expect(tile(0).querySelector('.stack')!.getAttribute('aria-label')).toBe('9 percent used, 9 percent by Pitwall');
    expect(tile(0).querySelector('.status')!.getAttribute('data-tone')).toBe('bad');
  });

  it('shows GPU memory with the name shortened and the model share', async () => {
    const { tile } = await mount((o) => ({ ...o, memory: mem(), models: [] }));
    const t = tile(1);
    expect(t.querySelector('.gpu-name')!.textContent).toBe('RTX 4070');
    expect(text(t.querySelector('.value')!)).toBe('6.0GBof12GB');
    expect((t.querySelector('.meter span') as HTMLElement).style.width).toBe('50%');
    expect(t.querySelector('.meter')!.getAttribute('data-tone')).toBe('neutral');
    expect(text(t.querySelector('.foot')!)).toBe('Models0B6.0GBfree');
  });

  it('stays neutral until VRAM is nearly full, then warns, then turns to danger', async () => {
    const { tile } = await mount((o) => ({ ...o, memory: mem({ gpus: [gpu({ vramUsed: 10.5 * GiB })] }) }));
    expect(tile(1).querySelector('.meter')!.getAttribute('data-tone')).toBe('warn');
    TestBed.resetTestingModule();
    const full = await mount((o) => ({ ...o, memory: mem({ gpus: [gpu({ vramUsed: 11.7 * GiB })] }) }));
    expect(full.tile(1).querySelector('.meter')!.getAttribute('data-tone')).toBe('bad');
  });

  it('shortens AMD and Intel GPU names and keeps others', async () => {
    for (const [name, short] of [['AMD Radeon RX 7800 XT', 'RX 7800 XT'], ['Intel(R) Arc A770', 'Arc A770'], ['Apple M3', 'Apple M3']]) {
      TestBed.resetTestingModule();
      const { tile } = await mount((o) => ({ ...o, memory: mem({ gpus: [gpu({ name: name! })] }) }));
      expect(tile(1).querySelector('.gpu-name')!.textContent).toBe(short);
    }
  });

  it('explains unified memory and drivers that do not report VRAM', async () => {
    const unified = await mount((o) => ({ ...o, memory: mem({ gpus: [gpu({ vramTotal: null, unified: true })] }) }));
    expect(text(unified.tile(1))).toContain('Unifiedmemory');
    expect(text(unified.tile(1))).toContain('TheGPUsharessystemmemory.');
    unified.set((o) => ({ ...o, memory: mem({ gpus: [gpu({ vramTotal: null, unified: false })] }) }));
    await settle(unified.fixture);
    expect(text(unified.tile(1))).toContain('Notreported');
    expect(text(unified.tile(1))).toContain('ThedriverdoesnotreportVRAM.');
  });

  it('says there is no GPU when none is present', async () => {
    const { tile } = await mount((o) => ({ ...o, memory: mem({ gpus: [] }) }));
    expect(text(tile(1))).toContain('NoGPU');
    expect(text(tile(1))).toContain('ModelsrunontheCPU.');
    expect(text(tile(2))).toContain('CPU');
    expect(tile(2).querySelector('.bar-row')).toBeNull();
  });

  it('shows CPU and GPU load, commit charge and swap', async () => {
    const { tile } = await mount((o) => ({ ...o, memory: mem() }));
    const t = tile(2);
    expect(text(t.querySelector('.value')!)).toBe('40%GPU');
    expect(Array.from(t.querySelectorAll('.bar-row')).map(text)).toEqual(['CPU24%']);
    expect(t.querySelector('.bar-row .meter')!.getAttribute('data-tone')).toBe('neutral');
    expect(text(t.querySelector('.foot')!)).toBe('Commit50%swap1.0GB');
    expect(t.querySelector('.status')).toBeNull();
  });

  it('shows an unknown commit charge and GPU load as dashes, and yielding to a full-screen app', async () => {
    const { tile } = await mount((o) => ({ ...o, memory: mem({ commitUsed: null, commitLimit: null, foregroundBusy: true, gpus: [gpu({ utilisation: null })] }) }));
    const t = tile(2);
    expect(text(t.querySelector('.foot')!)).toContain('Commit—');
    expect(text(t.querySelector('.value')!)).toBe('24%CPU');
    expect(t.querySelector('.status')!.textContent).toBe('Yielding');
  });

  it('shows throughput with the active request count', async () => {
    const throughput: Throughput = { genTps: 58.4, promptTps: 1184, ttftP50Ms: 214, requestsPerMin: 37, activeRequests: 3 };
    const { tile } = await mount((o) => ({ ...o, throughput }));
    const t = tile(3);
    expect(text(t.querySelector('.value')!)).toBe('58.4tok/sgeneration');
    expect(text(t.querySelector('.trio')!)).toBe('Prompt1,184tok/sTTFTp50214msRequests37/min');
    expect(t.querySelector('.gpu-name')!.textContent).toBe('3 active');
    expect(t.querySelector('.pill')).toBeNull();
  });

  it('says nothing about activity when idle and shows Paused while serving is paused', async () => {
    const throughput: Throughput = { genTps: 0, promptTps: 0, ttftP50Ms: 0, requestsPerMin: 0, activeRequests: 0 };
    const { tile, set, fixture, live } = await mount((o) => ({ ...o, throughput }));
    expect(tile(3).querySelector('.gpu-name')).toBeNull();
    expect(tile(3).querySelector('.status')).toBeNull();
    set((o) => ({ ...o, throughput, server: { ...o.server, serving: 'paused' } }));
    await settle(fixture);
    expect(live.paused()).toBe(true);
    expect(tile(3).querySelector('.status')!.textContent).toBe('Paused');
    expect(tile(3).querySelector('.status')!.getAttribute('data-tone')).toBe('warn');
  });
});
