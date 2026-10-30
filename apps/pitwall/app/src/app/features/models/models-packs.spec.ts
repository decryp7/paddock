import { dialogsAnswering } from '../../../testing/render';
import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import type { PitwallBackend } from '../../core/backend';
import { BACKEND } from '../../core/backend';
import { LiveStore } from '../../core/live.store';
import { MockBackend } from '../../core/mock-backend';
import { Toasts } from '../../core/toasts';
import { registerIcons } from '../../shared/icons';
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

  it('downloads a recommendation (with the projector of a vision model) and reports failures', async () => {
    const backend = new MockBackend();
    const { store, toasts } = storeOf(backend);
    await store.loadPacks();
    const gemma = store.packs()!.recommendations.find((r) => r.id === 'gemma-4-12b')!;
    expect(await store.downloadRecommendation(gemma)).toBe(true);
    const d = (await backend.downloads())[0]!;
    expect(d.name).toBe(gemma.file);
    expect(d.bytesTotal).toBeGreaterThan(gemma.sizeBytes);
    const qwen = store.packs()!.recommendations.find((r) => r.id === 'qwen3.5-2b')!;
    await store.downloadRecommendation(qwen);
    expect((await backend.downloads())[0]!.bytesTotal).toBeLessThan(qwen.sizeBytes + 500 * 1024 ** 2);

    expect(toasts.toasts().map((t) => t.tone)).toEqual(['ok', 'ok']);
    TestBed.resetTestingModule();
    const failing = storeOf(new MockBackend({ failing: ['repoFiles'] }));
    expect(await failing.store.downloadRecommendation(qwen)).toBe(false);
    expect(failing.toasts.toasts().map((t) => t.tone)).toEqual(['bad']);
  });
});

describe('Models page: recommendations and engines', () => {
  async function render(backend: PitwallBackend = new MockBackend()) {
    TestBed.configureTestingModule({
      imports: [ModelsPage],
      providers: [
        { provide: BACKEND, useValue: backend },
        dialogsAnswering(true),
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
    const chip = (name: string) => Array.from(r.el.querySelectorAll<HTMLElement>('pw-discover-tab mat-chip-option')).find((c) => c.textContent?.trim() === name)!;
    chip('Code').querySelector<HTMLElement>('[role=option]')!.click();
    await r.settle();
    expect(rows().length).toBeLessThan(all);
    expect(chip('Code').querySelector('[role=option]')!.getAttribute('aria-selected')).toBe('true');
    (r.el.querySelector('button[aria-label^="Download Qwen3.6 35B-A3B"]') as HTMLButtonElement).click();
    await r.settle();
    expect((await backend.downloads())[0]!.name).toContain('Qwen3.6-35B-A3B');
  });

  it('says so when no recommendation has the tag, and shows an inline error with Retry', async () => {
    const r = await render(new MockBackend({ failing: ['packs'] }));
    await discover(r);
    expect(r.el.querySelector('pw-discover-tab .recs [role=alert]')?.textContent).toContain('not responding');
  });

  it('has no per-model engine choice any more (llama.cpp serves everything), and shows skipped catalog files', async () => {
    const r = await render(new MockBackend());
    r.el.querySelector<HTMLElement>('#row-gemma-4-12b-it-q4_k_m')!.click();
    await r.settle();
    expect(r.el.querySelector('[id^="engine-"]')).toBeNull();
    await discover(r);
    const notice = r.el.querySelector('pw-discover-tab .pack-notice')!;
    expect(notice.textContent).toContain('1 catalog file was skipped');
    expect(notice.textContent).toContain('packs/old.models.toml');
    expect(notice.textContent).toContain('unknown key');
  });
});
