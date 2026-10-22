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
import { ModelsPage } from './models-page';
import { ModelsStore } from './models.store';

const tick = async (n = 6) => {
  for (let i = 0; i < n; i++) await new Promise((r) => setTimeout(r, 0));
};

function storeOf(backend: PitwallBackend = new MockBackend()) {
  TestBed.configureTestingModule({ providers: [ModelsStore, { provide: BACKEND, useValue: backend }] });
  return { store: TestBed.inject(ModelsStore), toasts: TestBed.inject(Toasts), backend };
}

describe('ModelsStore packs', () => {
  it('loads recommendations and remembers the tags seen without a filter', async () => {
    const { store } = storeOf();
    await store.loadPacks();
    expect(store.packs()?.recommendations.length).toBeGreaterThan(3);
    expect(store.packTags()).toEqual(expect.arrayContaining(['code', 'small', 'chat']));
    await store.loadPacks('code');
    expect(store.packTag()).toBe('code');
    expect(store.packs()?.recommendations.every((r) => r.tags.includes('code'))).toBe(true);
    // The chips stay the unfiltered set.
    expect(store.packTags()).toContain('small');
  });

  it('shows a pack load error and recovers on retry', async () => {
    const { store } = storeOf(new MockBackend({ failing: ['packs'] }));
    await store.loadPacks();
    expect(store.packsError()?.code).toBe('serverUnavailable');
    expect(store.packs()).toBeNull();
  });

  it('only offers installed pack engines as per-model choices', async () => {
    const backend = new MockBackend();
    const { store } = storeOf(backend);
    await store.loadPacks();
    await store.loadInstalledPacks();
    expect(store.engineChoices()).toEqual([]);
    await backend.installBuild('ik-llama-cpu');
    for (let i = 0; i < 400; i++) (backend as MockBackend).tick();
    await store.loadInstalledPacks();
    expect(store.engineChoices().map((e) => e.id)).toEqual(['ik-llama-cpu']);
  });

  it('tolerates a failing engines call while loading installed packs', async () => {
    const { store } = storeOf(new MockBackend({ failing: ['engines'] }));
    await store.loadInstalledPacks();
    expect(store.installedPacks()).toEqual([]);
  });

  it('downloads a recommendation (with the projector of a vision model) and reports failures', async () => {
    const backend = new MockBackend();
    const { store, toasts } = storeOf(backend);
    await store.loadPacks();
    const gemma = store.packs()!.recommendations.find((r) => r.id === 'gemma-3-12b')!;
    expect(await store.downloadRecommendation(gemma)).toBe(true);
    const d = (await backend.downloads())[0]!;
    expect(d.name).toBe(gemma.file);
    expect(d.bytesTotal).toBeGreaterThan(gemma.sizeBytes);
    const qwen = store.packs()!.recommendations.find((r) => r.id === 'qwen3-1.7b')!;
    await store.downloadRecommendation(qwen);
    expect((await backend.downloads())[0]!.bytesTotal).toBeLessThan(qwen.sizeBytes + 500 * 1024 ** 2);

    expect(toasts.toasts().map((t) => t.tone)).toEqual(['ok', 'ok']);
    TestBed.resetTestingModule();
    const failing = storeOf(new MockBackend({ failing: ['repoFiles'] }));
    expect(await failing.store.downloadRecommendation(qwen)).toBe(false);
    expect(failing.toasts.toasts().map((t) => t.tone)).toEqual(['bad']);
  });

  it('sets and clears a model engine, remembering the choice', async () => {
    const backend = new MockBackend();
    const { store } = storeOf(backend);
    await store.load();
    const id = store.library()[0]!.id;
    await store.setEngine(id, 'ik-llama-cpu');
    expect(store.library()[0]!.engine).toBe('ik-llama-cpu');
    await store.setEngine(id, null);
    expect(store.library()[0]!.engine).toBeNull();
    await store.setEngine('nope', 'ik-llama-cpu');
    expect(store.library().some((m) => m.engine === 'ik-llama-cpu')).toBe(false);
  });
});

describe('Models page: recommendations and engines', () => {
  async function render(backend: PitwallBackend = new MockBackend()) {
    TestBed.configureTestingModule({
      imports: [ModelsPage],
      providers: [
        { provide: BACKEND, useValue: backend },
        { provide: Confirm, useValue: { ask: () => Promise.resolve(true) } },
      ],
    });
    registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
    await TestBed.inject(LiveStore).load();
    const fixture = TestBed.createComponent(ModelsPage);
    fixture.detectChanges();
    await tick();
    fixture.detectChanges();
    const el = fixture.nativeElement as HTMLElement;
    const settle = async () => {
      await tick();
      fixture.detectChanges();
      await tick();
      fixture.detectChanges();
    };
    return { el, settle, fixture };
  }

  const discover = async (r: Awaited<ReturnType<typeof render>>) => {
    (Array.from(r.el.querySelectorAll('[role=tab]')).find((t) => t.textContent?.includes('Discover')) as HTMLElement).click();
    await r.settle();
  };

  it('lists recommendations with their fit, filters by tag chip and downloads in one click', async () => {
    const backend = new MockBackend();
    const r = await render(backend);
    await discover(r);
    expect(r.el.textContent).toContain('Recommended for this machine');
    const rows = () => r.el.querySelectorAll('pw-discover-tab .rec');
    const all = rows().length;
    expect(all).toBeGreaterThan(3);
    const chip = (name: string) => Array.from(r.el.querySelectorAll<HTMLButtonElement>('pw-discover-tab .chip')).find((c) => c.textContent?.trim() === name)!;
    chip('code').click();
    await r.settle();
    expect(rows().length).toBeLessThan(all);
    expect(chip('code').getAttribute('aria-pressed')).toBe('true');
    (r.el.querySelector('button[aria-label^="Download Qwen2.5 Coder"]') as HTMLButtonElement).click();
    await r.settle();
    expect((await backend.downloads())[0]!.name).toContain('Qwen2.5-Coder-7B');
  });

  it('says so when no recommendation has the tag, and shows an inline error with Retry', async () => {
    const r = await render(new MockBackend({ failing: ['packs'] }));
    await discover(r);
    expect(r.el.querySelector('pw-discover-tab .recs [role=alert]')?.textContent).toContain('not responding');
  });

  it('offers a per-model engine select only once a pack engine is installed', async () => {
    const backend = new MockBackend();
    let r = await render(backend);
    expect(r.el.querySelector('select[id^="engine-"]')).toBeNull();
    TestBed.resetTestingModule();
    await backend.installBuild('ik-llama-cpu');
    for (let i = 0; i < 400; i++) backend.tick();
    r = await render(backend);
    const select = r.el.querySelector<HTMLSelectElement>('select[id^="engine-"]')!;
    expect(select).not.toBeNull();
    select.value = 'ik-llama-cpu';
    select.dispatchEvent(new Event('change'));
    await r.settle();
    const id = select.id.replace('engine-', '');
    expect((await backend.localModels()).find((m) => m.id === id)?.engine).toBe('ik-llama-cpu');
  });
});
