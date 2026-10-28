import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import { provideRouter } from '@angular/router';
import { patchState, type WritableStateSource } from '@ngrx/signals';
import type { LoadedModel } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { LiveStore } from '../../core/live.store';
import { MockBackend } from '../../core/mock-backend';
import { registerIcons } from '../../shared/icons';
import { settle } from '../../../testing/render';
import { HotModels, idleProgress } from './hot-models';

const NOW = 1_800_000_000_000;
const model = (over: Partial<LoadedModel> = {}): LoadedModel => ({
  modelId: 'm',
  name: 'M',
  engine: 'llama.cpp',
  build: 'cpu',
  state: 'ready',
  ramBytes: 1024 ** 3,
  vramBytes: 0,
  contextTokens: 4096,
  slots: 2,
  pinned: false,
  activeRequests: 0,
  loadedAt: NOW - 60_000,
  lastUsedAt: NOW - 10_000,
  idleReleaseAt: NOW + 10_000,
  coldStartMs: 100,
  capabilities: ['chat'],
  ...over,
});

describe('idleProgress', () => {
  it('is null when the model is never released automatically', () => {
    expect(idleProgress(model({ idleReleaseAt: null }), NOW)).toBeNull();
  });
  it('runs from 0 (just used) to 1 (due)', () => {
    expect(idleProgress(model({ lastUsedAt: NOW, idleReleaseAt: NOW + 100 }), NOW)).toBe(0);
    expect(idleProgress(model({ lastUsedAt: NOW - 50, idleReleaseAt: NOW + 50 }), NOW)).toBe(0.5);
    expect(idleProgress(model({ lastUsedAt: NOW - 500, idleReleaseAt: NOW - 100 }), NOW)).toBe(1);
  });
  it('clamps to 0 when the clock is before the last use', () => {
    expect(idleProgress(model({ lastUsedAt: NOW + 50, idleReleaseAt: NOW + 150 }), NOW)).toBe(0);
  });
  it('is 1 for a zero or negative idle span', () => {
    expect(idleProgress(model({ lastUsedAt: NOW, idleReleaseAt: NOW }), NOW)).toBe(1);
    expect(idleProgress(model({ lastUsedAt: NOW + 5, idleReleaseAt: NOW }), NOW)).toBe(1);
  });
});

async function mountWith(models: LoadedModel[], inputs: { now?: number; compact?: boolean } = {}, backend = new MockBackend({ now: () => NOW })) {
  TestBed.configureTestingModule({ imports: [HotModels], providers: [{ provide: BACKEND, useValue: backend }, provideRouter([])] });
  registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
  const live = TestBed.inject(LiveStore);
  await live.load();
  patchState(live as unknown as WritableStateSource<object>, { overview: { ...live.overview()!, models } });
  const fixture = TestBed.createComponent(HotModels);
  fixture.componentRef.setInput('now', inputs.now ?? NOW);
  fixture.componentRef.setInput('compact', inputs.compact ?? false);
  await settle(fixture);
  return { fixture, el: fixture.nativeElement as HTMLElement, live, backend };
}

describe('HotModels', () => {
  it('lists the hot models with state, memory, context and countdown', async () => {
    const { el } = await mountWith([model({ modelId: 'a', name: 'Alpha', vramBytes: 2 * 1024 ** 3, contextTokens: 16384, lastUsedAt: NOW - 10_000, idleReleaseAt: NOW + 10_000 })]);
    const row = el.querySelector('li.row')!;
    expect(row.querySelector('.name')!.textContent).toBe('Alpha');
    expect(row.querySelector('.state')!.getAttribute('aria-label')).toBe('Ready');
    expect(row.querySelector('.meta')!.textContent).toContain('2 GB VRAM');
    expect(row.querySelector('.meta')!.textContent).toContain('1 GB RAM');
    // Context and slots are secondary: they are in the row's tooltip, not on its line.
    expect(row.querySelector('.meta')!.textContent).not.toContain('ctx');
    expect(row.querySelector('.when')!.getAttribute('aria-label')).toBe('Released in 0:10');
    expect((row.querySelector('.idle span') as HTMLElement).style.width).toBe('50%');
  });

  it('shows the empty state with a link to the library, or a bare one when compact', async () => {
    const full = await mountWith([]);
    expect(full.el.querySelector('pw-empty-state')!.textContent).toContain('No model loaded');
    expect(full.el.querySelector('a[href="/models"]')).not.toBeNull();
    TestBed.resetTestingModule();
    const compact = await mountWith([], { compact: true });
    expect(compact.el.querySelector('pw-empty-state')!.textContent).toContain('No model loaded');
    expect(compact.el.querySelector('a[href="/models"]')).toBeNull();
  });

  it('omits VRAM when zero', async () => {
    const { el } = await mountWith([model({ vramBytes: 0, contextTokens: 512 })]);
    const meta = el.querySelector('.meta')!.textContent!;
    expect(meta).not.toContain('VRAM');
    expect(meta).toContain('RAM');
  });

  it('shows request counts for busy models, with singular and plural', async () => {
    const { el } = await mountWith([
      model({ modelId: 'a', name: 'A', state: 'busy', activeRequests: 1 }),
      model({ modelId: 'b', name: 'B', state: 'ready', activeRequests: 2 }),
      model({ modelId: 'c', name: 'C', state: 'busy', activeRequests: 0 }),
    ]);
    const whens = Array.from(el.querySelectorAll('.when')).map((w) => w.textContent!.trim());
    expect(whens).toEqual(['1 request', '2 requests', '0 requests']);
  });

  it('marks pinned models and shows Pinned instead of a countdown', async () => {
    const { el } = await mountWith([model({ pinned: true, idleReleaseAt: null })]);
    expect(el.querySelector('mat-icon.pinned')).not.toBeNull();
    expect(el.querySelector('.when')!.textContent).toBe('Pinned');
    expect(el.querySelector('button[aria-label="Unpin M"]')).not.toBeNull();
  });

  it('falls back to the state label when there is no release time and no pin', async () => {
    const { el } = await mountWith([model({ state: 'loading', idleReleaseAt: null })]);
    expect(el.querySelector('.when')!.textContent).toBe('Loading');
  });

  it('pins and unpins in place through the live store', async () => {
    const { fixture, el, live, backend } = await mountWith([model()]);
    const spy = vi.spyOn(backend, 'pinModel');
    (el.querySelector('button[aria-label="Pin M"]') as HTMLButtonElement).click();
    await settle(fixture);
    expect(spy).toHaveBeenCalledWith('m', true);
    expect(live.isBusy()('model:m')).toBe(false);
  });

  it('unloads a model through the live store and disables the buttons meanwhile', async () => {
    const { fixture, el, live, backend } = await mountWith([model()]);
    const spy = vi.spyOn(backend, 'unloadModel');
    const unload = el.querySelector('button[aria-label="Unload M"]') as HTMLButtonElement;
    unload.click();
    fixture.detectChanges();
    expect(unload.disabled).toBe(true);
    expect((el.querySelector('button[aria-label="Pin M"]') as HTMLButtonElement).disabled).toBe(true);
    await settle(fixture);
    expect(spy).toHaveBeenCalledWith('m');
    expect(live.isBusy()('model:m')).toBe(false);
  });

  it('is read-only in compact mode: no actions, no context, no GPU split', async () => {
    const { el } = await mountWith([model({ vramBytes: 1024 ** 3 })], { compact: true });
    expect(el.querySelector('.actions')).toBeNull();
    expect(el.querySelector('ul.list')!.classList.contains('compact')).toBe(true);
    expect(el.querySelector('.meta')!.textContent).not.toContain('ctx');
    expect(el.querySelector('.split')).toBeNull();
  });

  it('splits GPU memory by model plus other apps', async () => {
    const { el } = await mountWith([model({ modelId: 'a', name: 'Alpha', vramBytes: 2 * 1024 ** 3 }), model({ modelId: 'b', name: 'Beta', vramBytes: 0 })]);
    const bar = el.querySelector('.split-bar')!;
    expect(bar.getAttribute('aria-label')).toMatch(/^GPU memory: Alpha \d+%, Other apps \d+%$/);
    expect(bar.querySelectorAll('span').length).toBe(2);
    expect(el.querySelector('.split-legend')!.textContent).toContain('Alpha');
    expect(el.querySelector('.split-legend')!.textContent).toContain('Other apps');
    expect(el.querySelector('.split-legend')!.textContent).not.toContain('Beta');
  });

  it('has no GPU split without models, without a GPU or when VRAM is not reported', async () => {
    const none = await mountWith([]);
    expect(none.el.querySelector('.split')).toBeNull();
    const live = TestBed.inject(LiveStore);
    const o = live.overview()!;
    patchState(live as unknown as WritableStateSource<object>, { overview: { ...o, models: [model({ vramBytes: 1 })], memory: { ...o.memory, gpus: [] } } });
    await settle(none.fixture);
    expect(none.el.querySelector('.split')).toBeNull();
    patchState(live as unknown as WritableStateSource<object>, { overview: { ...o, models: [model({ vramBytes: 1 })], memory: { ...o.memory, gpus: [{ ...o.memory.gpus[0]!, vramUsed: null }] } } });
    await settle(none.fixture);
    expect(none.el.querySelector('.split')).toBeNull();
  });

  it('does not show an Other apps segment when models use all the used VRAM', async () => {
    const { el, live, fixture } = await mountWith([model()]);
    const o = live.overview()!;
    const used = 3 * 1024 ** 3;
    patchState(live as unknown as WritableStateSource<object>, { overview: { ...o, models: [model({ vramBytes: used })], memory: { ...o.memory, gpus: [{ ...o.memory.gpus[0]!, vramUsed: used }] } } });
    await settle(fixture);
    expect(el.querySelectorAll('.split-bar span').length).toBe(1);
  });
});
