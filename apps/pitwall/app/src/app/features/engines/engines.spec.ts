import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import type { PitwallBackend } from '../../core/backend';
import { BACKEND } from '../../core/backend';
import { LiveStore } from '../../core/live.store';
import { MockBackend } from '../../core/mock-backend';
import { Toasts } from '../../core/toasts';
import { registerIcons } from '../../shared/icons';
import { Confirm } from '../../shared/ui/confirm';
import { modelLabel } from './engine-card';
import { EnginesPage } from './engines-page';
import { EnginesStore } from './engines.store';

const tick = async (n = 6) => {
  for (let i = 0; i < n; i++) await new Promise((r) => setTimeout(r, 0));
};

describe('EnginesStore', () => {
  function setup(backend: PitwallBackend = new MockBackend()) {
    TestBed.configureTestingModule({ providers: [EnginesStore, { provide: BACKEND, useValue: backend }] });
    return { store: TestBed.inject(EnginesStore), toasts: TestBed.inject(Toasts) };
  }

  it('loads engines and splits available from planned ones', async () => {
    const { store } = setup();
    const p = store.load();
    expect(store.loading()).toBe(true);
    await p;
    expect(store.available().map((e) => e.kind)).toEqual(['llama.cpp']);
    expect(store.planned().map((e) => e.kind)).toEqual(['whisper.cpp', 'mlx', 'vllm']);
  });

  it('reports a load error', async () => {
    const { store } = setup(new MockBackend({ failing: ['engines'] }));
    await store.load();
    expect(store.error()?.code).toBe('serverUnavailable');
    expect(store.loaded()).toBe(false);
  });

  it('selects, installs, removes and rolls back builds, reloading after each', async () => {
    const backend = new MockBackend();
    const { store, toasts } = setup(backend);
    await store.load();
    expect(await store.select('vulkan')).toBe(true);
    expect(store.available()[0]!.builds.find((b) => b.active)?.id).toBe('vulkan');
    expect(await store.select('auto')).toBe(true);
    expect(store.available()[0]!.buildChoice).toBe('auto');
    const cpu = store.available()[0]!.builds.find((b) => b.id === 'cpu')!;
    expect(await store.remove(cpu)).toBe(true);
    expect(store.available()[0]!.builds.some((b) => b.id === 'cpu')).toBe(false);
    expect(await store.install(store.available()[0]!.catalog[0]!)).toBe(true);
    expect((await backend.downloads())[0]!.kind).toBe('engine');
    expect(await store.rollback()).toBe(true);
    expect(store.available()[0]!.rollbackTo).toBeNull();
    expect(toasts.toasts().at(-1)?.tone).toBe('ok');
  });

  it('keeps the data and toasts the reason when an action fails, ignoring repeats in flight', async () => {
    const { store, toasts } = setup();
    await store.load();
    const active = store.available()[0]!.builds.find((b) => b.active)!;
    expect(await store.remove(active)).toBe(false);
    expect(toasts.toasts().at(-1)?.message).toContain('in use');
    expect(await store.rollback()).toBe(true);
    expect(await store.rollback()).toBe(false);
    expect(toasts.toasts().at(-1)?.message).toContain('nothing to roll back');
    const a = store.select('vulkan');
    expect(store.isBusy()('select')).toBe(true);
    expect(await store.select('cpu')).toBe(false);
    await a;
  });
});

describe('EnginesPage', () => {
  async function render(backend: PitwallBackend = new MockBackend(), confirm = true) {
    TestBed.configureTestingModule({
      imports: [EnginesPage],
      providers: [
        { provide: BACKEND, useValue: backend },
        { provide: Confirm, useValue: { ask: () => Promise.resolve(confirm) } },
      ],
    });
    registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
    const fixture = TestBed.createComponent(EnginesPage);
    fixture.detectChanges();
    await tick();
    fixture.detectChanges();
    const settle = async () => {
      await tick();
      fixture.detectChanges();
      await tick();
      fixture.detectChanges();
    };
    return { el: fixture.nativeElement as HTMLElement, settle, live: TestBed.inject(LiveStore) };
  }

  it('names models in the process table', () => {
    expect(modelLabel('qwen3-8b-instruct-q4_k_m')).toBe('qwen3-8b-instruct');
    expect(modelLabel('nomic-embed-text-v1.5-f16')).toBe('nomic-embed-text-v1.5');
  });

  it('shows llama.cpp builds, the catalog with the suggested build, processes and planned engines', async () => {
    const { el } = await render();
    expect(el.querySelector('h2#eng-llama\\.cpp')?.textContent).toContain('llama.cpp');
    expect(el.textContent).toContain('CUDA 13 (NVIDIA)');
    expect(el.textContent).toContain('Self-test passed');
    expect(el.textContent).toContain('Not tested');
    expect(el.querySelector('.build.suggested')?.textContent).toContain('Suggested');
    expect(el.textContent).toContain('Roll back to b11602');
    expect(el.textContent).toContain('Healthy');
    const planned = el.querySelectorAll('.planned li');
    expect(planned.length).toBe(3);
    expect(planned[1]!.textContent).toContain('MLX runs on Apple silicon only');
    expect((el.querySelector('button[aria-label="Remove CUDA 13 (NVIDIA) b11669"]') as HTMLButtonElement).disabled).toBe(true);
  });

  it('chooses a build, installs, removes and rolls back', async () => {
    const backend = new MockBackend();
    const { el, settle, live } = await render(backend);
    await live.load();
    const select = el.querySelector('#build-choice') as HTMLSelectElement;
    select.value = 'vulkan';
    select.dispatchEvent(new Event('change'));
    await settle();
    expect((await backend.engines())[0]!.buildChoice).toBe('vulkan');

    (el.querySelector('button[aria-label="Install CUDA 13 (NVIDIA) b11702"]') as HTMLButtonElement).click();
    await settle();
    live.connect();
    backend.tick();
    await settle();
    expect(el.querySelector('.installing')?.textContent).toContain('llama.cpp CUDA 13');
    for (let i = 0; i < 6; i++) backend.tick();
    await settle();
    expect(el.textContent).toContain('b11702');
    live.disconnect();

    (el.querySelector('button[aria-label="Remove CPU (AVX2) b11602"]') as HTMLButtonElement).click();
    await settle();
    expect(el.querySelector('button[aria-label="Remove CPU (AVX2) b11602"]')).toBeNull();

    const rollback = Array.from(el.querySelectorAll('button')).find((b) => b.textContent?.includes('Roll back'))!;
    rollback.click();
    await settle();
    expect(el.textContent).not.toContain('Roll back to');
  });

  it('does nothing when a confirmation is declined', async () => {
    const { el, settle } = await render(new MockBackend(), false);
    (el.querySelector('button[aria-label="Remove CPU (AVX2) b11602"]') as HTMLButtonElement).click();
    Array.from(el.querySelectorAll('button'))
      .find((b) => b.textContent?.includes('Roll back'))!
      .click();
    await settle();
    expect(el.querySelector('button[aria-label="Remove CPU (AVX2) b11602"]')).not.toBeNull();
    expect(el.textContent).toContain('Roll back to b11602');
  });

  it('shows an error with Retry, and Retry loads again', async () => {
    let fail = true;
    const mock = new MockBackend();
    const backend = new Proxy(mock, {
      get: (t, k) => (k === 'engines' ? () => (fail ? Promise.reject({ code: 'serverUnavailable', message: 'Server down' }) : mock.engines()) : Reflect.get(t, k, t)),
    }) as PitwallBackend;
    const { el, settle } = await render(backend);
    expect(el.querySelector('[role="alert"]')?.textContent).toContain('Server down');
    fail = false;
    Array.from(el.querySelectorAll('button'))
      .find((b) => b.textContent?.trim() === 'Retry')!
      .click();
    await settle();
    expect(el.querySelector('[role="alert"]')).toBeNull();
    expect(el.textContent).toContain('CUDA 13 (NVIDIA)');
  });
});
