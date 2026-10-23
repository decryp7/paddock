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

  it('loads llama.cpp, the only engine', async () => {
    const { store } = setup();
    const p = store.load();
    expect(store.loading()).toBe(true);
    await p;
    expect(store.engines().map((e) => e.kind)).toEqual(['llama.cpp']);
    expect(store.llama()?.name).toBe('llama.cpp');
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
    expect(store.llama()!.builds.find((b) => b.active)?.id).toBe('vulkan');
    expect(await store.select('auto')).toBe(true);
    expect(store.llama()!.buildChoice).toBe('auto');
    const cpu = store.llama()!.builds.find((b) => b.id === 'cpu')!;
    expect(await store.remove(cpu)).toBe(true);
    expect(store.llama()!.builds.some((b) => b.id === 'cpu')).toBe(false);
    expect(await store.install(store.llama()!.catalog[0]!)).toBe(true);
    expect((await backend.downloads())[0]!.kind).toBe('engine');
    expect(await store.rollback()).toBe(true);
    expect(store.pins()?.global).toBe('vulkan');
    expect(toasts.toasts().at(-1)?.message).toContain('Rolled back to vulkan b11669');
    expect(toasts.toasts().at(-1)?.tone).toBe('ok');
  });

  it('keeps the data and toasts the reason when an action fails, ignoring repeats in flight', async () => {
    const { store, toasts } = setup();
    await store.load();
    const active = store.llama()!.builds.find((b) => b.active)!;
    expect(await store.remove(active)).toBe(false);
    expect(toasts.toasts().at(-1)?.message).toContain('in use');
    const first = store.rollback();
    expect(await store.rollback()).toBe(false);
    expect(await first).toBe(true);
    expect(toasts.toasts().at(-1)?.tone).toBe('ok');
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

  /** Opens a build's "..." menu and runs one item. */
  async function menu(el: HTMLElement, settle: () => Promise<void>, build: string, item: string) {
    (el.querySelector(`button[aria-label="More actions for ${build}"]`) as HTMLButtonElement).click();
    await settle();
    Array.from(document.querySelectorAll<HTMLButtonElement>('.pw-menu .pw-menu-item')).find((b) => b.textContent!.trim() === item)!.click();
    await settle();
  }

  it('names models in the process table', () => {
    expect(modelLabel('qwen3.5-9b-q4_k_m')).toBe('qwen3.5-9b');
    expect(modelLabel('qwen3-embedding-0.6b-q8_0')).toBe('qwen3-embedding-0.6b');
  });

  it('shows llama.cpp builds, the catalog with the suggested build and processes, and no other engines', async () => {
    const { el } = await render();
    expect(el.querySelector('h1')?.textContent).toContain('llama.cpp');
    expect(el.textContent).toContain('CUDA 13 (NVIDIA)');
    expect(el.textContent).toContain('Self-test passed');
    expect(el.textContent).toContain('Not tested');
    expect(el.querySelector('.catalog')!.querySelectorAll('li').length).toBe(4);
    expect(el.querySelector('.catalog li .rec')?.textContent).toContain('Suggested');
    expect(Array.from(el.querySelectorAll('.catalog button')).every((b) => !b.classList.contains('primary'))).toBe(true);
    expect(el.textContent).not.toContain('Healthy');
    expect(el.querySelector('.tag.upd')).toBeNull();
    expect(el.querySelector('.section-head .link')?.textContent).toContain('Update available');
    expect(el.textContent).toContain('Roll back to Vulkan b11669');
    expect(el.querySelector('.planned')).toBeNull();
    expect(el.textContent).not.toContain('MLX');
    expect(el.textContent).not.toContain('Planned');
    // Remove is only in the row menu: nothing destructive at rest.
    expect(el.querySelector('button[aria-label="Remove CUDA 13 (NVIDIA) b11669"]')).toBeNull();
  });

  it('chooses a build, installs, removes and rolls back', async () => {
    const backend = new MockBackend();
    const { el, settle, live } = await render(backend);
    await live.load();
    expect(el.querySelector('select')).toBeNull();
    (el.querySelector('#build-choice') as HTMLButtonElement).click();
    await settle();
    const opts = Array.from(document.querySelectorAll<HTMLElement>('.cdk-overlay-container .pw-select-list .opt'));
    expect(opts[0]!.textContent).toContain('Automatic');
    opts.find((o) => o.textContent!.includes('Vulkan'))!.click();
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

    await menu(el, settle, 'CPU (AVX2) b4300', 'Remove…');
    expect(el.querySelector('button[aria-label="More actions for CPU (AVX2) b4300"]')).toBeNull();

    const rollback = Array.from(el.querySelectorAll('button')).find((b) => b.textContent?.includes('Roll back'))!;
    rollback.click();
    await settle();
    // Vulkan was chosen first, so roll back went to the CUDA build, pinned it, and now offers Vulkan as the way back.
    expect(el.textContent).toContain('Pinned to CUDA 13 (NVIDIA) b11702 for all models');
  });

  it('does nothing when a confirmation is declined', async () => {
    const { el, settle } = await render(new MockBackend(), false);
    await menu(el, settle, 'CPU (AVX2) b4300', 'Remove…');
    Array.from(el.querySelectorAll('button'))
      .find((b) => b.textContent?.includes('Roll back'))!
      .click();
    await settle();
    expect(el.querySelector('button[aria-label="More actions for CPU (AVX2) b4300"]')).not.toBeNull();
    expect(el.textContent).toContain('Roll back to Vulkan b11669');
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

  it('offers the catalog with Install, suggested build first, when no llama.cpp build is installed', async () => {
    const backend = new MockBackend();
    const real = await backend.engines();
    const llama = structuredClone(real[0]!);
    llama.builds = [];
    llama.availability = { state: 'notInstalled', reason: 'No llama.cpp build is installed yet.' };
    llama.catalog.forEach((c) => (c.installed = false));
    backend.engines = () => Promise.resolve([llama]);
    const { el } = await render(backend);
    expect(el.querySelector('h1')?.textContent).toContain('llama.cpp');
    expect(el.querySelector('.planned')).toBeNull();
    const rows = Array.from(el.querySelectorAll('.catalog li'));
    expect(rows.length).toBe(llama.catalog.length);
    expect(rows[0]!.querySelector('.rec')?.textContent).toContain('Suggested');
    expect(rows[0]!.querySelector('button')?.textContent).toContain('Install');
    expect(el.textContent).not.toContain('Open the llama.cpp page');
    expect(el.textContent).toContain('installs the build suggested');
  });

  it('offers the leftover llamafile for one-click removal and never removes it by itself', async () => {
    const backend = new MockBackend();
    const { el, settle } = await render(backend);
    const row = el.querySelector('.leftovers li')!;
    expect(row.textContent).toContain('llamafile');
    expect(row.textContent).toContain('Nothing is deleted until you remove it');
    expect((await backend.engines())[0]!.leftovers.length).toBe(1);
    const btn = row.querySelector('button')!;
    expect(btn.textContent).toContain('Remove llamafile (351 MB)');
    btn.click();
    await settle();
    expect((await backend.engines())[0]!.leftovers).toEqual([]);
    expect(el.querySelector('.leftovers')).toBeNull();
  });
});
