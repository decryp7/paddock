import type { EngineStatusTile, Overview } from '../../core/api.types';
import { TestBed } from '@angular/core/testing';
import { patchState, type WritableStateSource } from '@ngrx/signals';
import { BACKEND } from '../../core/backend';
import { LiveStore } from '../../core/live.store';
import { MockBackend } from '../../core/mock-backend';
import { settle } from '../../../testing/render';

import { EnginesCard } from './engines-card';

const tile = (over: Partial<EngineStatusTile>): EngineStatusTile => ({ kind: 'llama.cpp', name: 'llama.cpp', availability: { state: 'available' }, activeBuild: 'cuda-13 · b1', processes: 1, unhealthy: 0, ...over });

async function mount(patch?: (o: Overview) => Overview, backend = new MockBackend()) {
  TestBed.configureTestingModule({ imports: [EnginesCard], providers: [{ provide: BACKEND, useValue: backend }] });
  const live = TestBed.inject(LiveStore);
  await live.load();
  if (patch) patchState(live as unknown as WritableStateSource<object>, { overview: patch(live.overview()!) });
  const fixture = TestBed.createComponent(EnginesCard);
  await settle(fixture);
  return { fixture, el: fixture.nativeElement as HTMLElement };
}

describe('EnginesCard', () => {
  it('is empty before the overview loads', async () => {
    const { el } = await mount(undefined, new MockBackend({ failing: ['overview'] }));
    expect(el.querySelectorAll('li').length).toBe(0);
  });

  it('shows the active build and process count of an available engine', async () => {
    const { el } = await mount();
    const first = el.querySelector('li')!;
    expect(first.querySelector('.name')!.textContent).toBe('llama.cpp');
    expect(first.querySelector('.sub')!.textContent!.replace(/\s+/g, ' ').trim()).toMatch(/^cuda-13 · b\d+ · \d+ processes?$/);
    expect(first.querySelector('.status')!.textContent).toBe('Available');
    expect(first.classList.contains('off')).toBe(false);
  });

  it('shows the reason an engine is unavailable and dims it', async () => {
    const { el } = await mount((o) => ({ ...o, engines: [tile({ availability: { state: 'notInstalled', reason: 'No llama.cpp build is installed yet.' }, activeBuild: null, processes: 0 })] }));
    const off = Array.from(el.querySelectorAll('li')).filter((l) => l.classList.contains('off'));
    expect(off.length).toBe(1);
    expect(off[0]!.querySelector('.sub')!.textContent).toContain('No llama.cpp build is installed yet.');
    expect(off[0]!.querySelector('.status')!.textContent).toMatch(/Not installed|Not available/);
  });

  it('uses the singular for one process and a placeholder for no build', async () => {
    const { el } = await mount((o) => ({ ...o, engines: [tile({ processes: 1 }), tile({ name: 'Second', activeBuild: null, processes: 0 })] }));
    const subs = Array.from(el.querySelectorAll('.sub')).map((s) => s.textContent!.replace(/\s+/g, ' ').trim());
    expect(subs).toEqual(['cuda-13 · b1 · 1 process', 'no build · 0 processes']);
  });

  it('flags unhealthy processes with a status', async () => {
    const { el } = await mount((o) => ({ ...o, engines: [tile({ unhealthy: 2 })] }));
    const pill = el.querySelector('.status')!;
    expect(pill.textContent).toBe('2 unhealthy');
    expect(pill.getAttribute('data-tone')).toBe('bad');
  });

  it('shows a failed engine with its reason', async () => {
    const { el } = await mount((o) => ({ ...o, engines: [tile({ availability: { state: 'failed', reason: 'Self-test failed' }, activeBuild: null, processes: 0 })] }));
    expect(el.querySelector('.sub')!.textContent!.trim()).toBe('Self-test failed');
    expect(el.querySelector('.status')!.textContent).toBe('Failed');
  });
});
