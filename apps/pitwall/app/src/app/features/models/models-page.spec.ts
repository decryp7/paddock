import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import type { Download, Fit } from '../../core/api.types';
import type { PitwallBackend } from '../../core/backend';
import { BACKEND } from '../../core/backend';
import { LiveStore } from '../../core/live.store';
import { MockBackend } from '../../core/mock-backend';
import { registerIcons } from '../../shared/icons';
import { Confirm } from '../../shared/ui/confirm';
import { fitDetail } from './discover-tab';
import { etaMs } from './downloads-list';
import { contextText, idleOptions, paramsText } from './library-tab';
import { ModelsPage, nextTab } from './models-page';

const tick = async (n = 6) => {
  for (let i = 0; i < n; i++) await new Promise((r) => setTimeout(r, 0));
};

async function render(backend: PitwallBackend = new MockBackend(), confirm = true) {
  TestBed.configureTestingModule({
    imports: [ModelsPage],
    providers: [
      { provide: BACKEND, useValue: backend },
      { provide: Confirm, useValue: { ask: () => Promise.resolve(confirm) } },
    ],
  });
  registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
  const live = TestBed.inject(LiveStore);
  await live.load();
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
  const button = (text: string) => Array.from(el.querySelectorAll('button')).find((b) => b.textContent?.trim().startsWith(text)) as HTMLButtonElement;
  return { fixture, el, settle, button, live };
}

describe('models helpers', () => {
  it('formats params, context and idle choices', () => {
    expect(paramsText(null)).toBe('—');
    expect(paramsText(8.2)).toBe('8.2B');
    expect(paramsText(12.2)).toBe('12B');
    expect(paramsText(0.14)).toBe('140M');
    expect(contextText(null)).toBe('—');
    expect(contextText(32768)).toBe('32k');
    expect(contextText(512)).toBe('512');
    expect(idleOptions(null)).toEqual([60, 120, 300, 600, 1800, 3600]);
    expect(idleOptions(900)).toContain(900);
    expect(idleOptions(300).length).toBe(6);
  });

  it('describes a fit and a time left', () => {
    const fit: Fit = { verdict: 'tight', needRam: 2 * 1024 ** 3, needVram: 3 * 1024 ** 3, haveRam: 0, haveVram: 0, offload: 'partial', message: '' };
    expect(fitDetail(fit)).toBe('3.0 GB VRAM + 2.0 GB RAM · part of the layers on the GPU');
    expect(fitDetail({ ...fit, needRam: 0, needVram: 0, offload: 'none' })).toBe('Nothing · CPU only');
    expect(fitDetail({ ...fit, needRam: 0, offload: 'all' })).toContain('all layers on the GPU');
    const d = { state: 'downloading', speedBps: 10, bytesDone: 0, bytesTotal: 100 } as Download;
    expect(etaMs(d)).toBe(10_000);
    expect(etaMs({ ...d, speedBps: 0 })).toBeNull();
    expect(etaMs({ ...d, state: 'paused' })).toBeNull();
  });

  it('moves between tabs with the arrow, Home and End keys', () => {
    expect(nextTab('library', 'ArrowRight')).toBe('discover');
    expect(nextTab('library', 'ArrowLeft')).toBe('downloads');
    expect(nextTab('downloads', 'ArrowRight')).toBe('library');
    expect(nextTab('discover', 'Home')).toBe('library');
    expect(nextTab('discover', 'End')).toBe('downloads');
    expect(nextTab('discover', 'a')).toBeNull();
  });
});

describe('ModelsPage', () => {
  it('lists the library with disk usage, status and actions', async () => {
    const { el } = await render();
    const rows = el.querySelectorAll('pw-library-tab tbody tr');
    expect(rows.length).toBe(6);
    expect(rows[0]!.textContent).toContain('Qwen3 8B Instruct');
    expect(rows[0]!.textContent).toContain('Loaded');
    expect(el.textContent).toContain('Partial downloads');
    expect(el.textContent).toContain('1 unused for 30+ days');
    expect(el.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toContain('Library');
    expect(el.querySelector('button[aria-label="Delete Gemma 3 12B"]')).not.toBeNull();
  });

  it('loads, pins, sets idle, deletes and cleans up from the table', async () => {
    const backend = new MockBackend();
    const { el, settle, button } = await render(backend);
    (el.querySelector('button[aria-label="Load Gemma 3 12B"]') as HTMLButtonElement).click();
    await settle();
    expect(el.querySelector('button[aria-label="Unload Gemma 3 12B"]')).not.toBeNull();
    (el.querySelector('button[aria-label="Pin Gemma 3 12B"]') as HTMLButtonElement).click();
    await settle();
    expect(el.querySelector('button[aria-label="Unpin Gemma 3 12B"]')).not.toBeNull();

    const select = el.querySelector('#idle-llama-3\\.2-3b-instruct-q5_k_m') as HTMLSelectElement;
    select.value = '600';
    select.dispatchEvent(new Event('change'));
    await settle();
    expect((await backend.localModels()).find((m) => m.id === 'llama-3.2-3b-instruct-q5_k_m')?.idleSecs).toBe(600);
    select.value = '';
    select.dispatchEvent(new Event('change'));
    await settle();
    expect((await backend.localModels()).find((m) => m.id === 'llama-3.2-3b-instruct-q5_k_m')?.idleSecs).toBeNull();

    (el.querySelector('button[aria-label="Delete BGE reranker v2 m3"]') as HTMLButtonElement).click();
    await settle();
    expect(el.textContent).not.toContain('BGE reranker v2 m3');
    button('Clean up unused').click();
    await settle();
    expect(el.textContent).not.toContain('Qwen2.5 Coder 1.5B');
  });

  it('does nothing when a confirmation is declined', async () => {
    const { el, settle, button } = await render(new MockBackend(), false);
    (el.querySelector('button[aria-label="Delete BGE reranker v2 m3"]') as HTMLButtonElement).click();
    button('Clean up unused').click();
    await settle();
    expect(el.textContent).toContain('BGE reranker v2 m3');
    expect(el.textContent).toContain('Qwen2.5 Coder 1.5B');
  });

  it('shows the empty library with a way to discover models', async () => {
    const { el, settle, button } = await render(new MockBackend({ empty: true }));
    expect(el.textContent).toContain('No models yet');
    button('Discover models').click();
    await settle();
    expect(el.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toContain('Discover');
  });

  it('shows an error with Retry when the library fails', async () => {
    const { el } = await render(new MockBackend({ failing: ['localModels'] }));
    expect(el.querySelector('[role="alert"]')?.textContent).toContain('not responding');
  });

  it('searches, opens a repo with fit verdicts and downloads a variant with its projector', async () => {
    const backend = new MockBackend();
    const { el, settle, fixture } = await render(backend);
    (el.querySelector('#tab-library') as HTMLElement).dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight' }));
    await settle();
    expect(el.querySelectorAll('pw-discover-tab .hit').length).toBe(12);
    const input = el.querySelector('#hf-q') as HTMLInputElement;
    input.value = 'gemma';
    (el.querySelector('form[role="search"]') as HTMLFormElement).dispatchEvent(new Event('submit'));
    await settle();
    const hits = el.querySelectorAll<HTMLButtonElement>('pw-discover-tab .hit');
    expect(hits.length).toBe(2);
    hits[0]!.click();
    await settle();
    expect(el.textContent).toContain('vision projector');
    const rows = el.querySelectorAll('pw-discover-tab tbody tr');
    expect(rows.length).toBe(8);
    expect(el.textContent).toContain('Tight');
    expect(el.textContent).toContain("Won't fit");
    expect(el.textContent).toContain('In library');
    (el.querySelector('pw-discover-tab input[type="checkbox"]') as HTMLInputElement).click();
    fixture.detectChanges();
    (el.querySelector('button[aria-label^="Download gemma-3-12b-it-Q2_K"]') as HTMLButtonElement).click();
    await settle();
    const d = (await backend.downloads())[0]!;
    expect(d.name).toBe('gemma-3-12b-it-Q2_K.gguf');
    (el.querySelector('button[aria-label="Close repo"]') as HTMLButtonElement).click();
    await settle();
    expect(el.textContent).toContain('Pick a repo');
  });

  it('shows downloads with progress, speed and the resumable state, and cancels them', async () => {
    const backend = new MockBackend();
    const { el, settle, live } = await render(backend);
    (el.querySelector('#tab-downloads') as HTMLButtonElement).click();
    await settle();
    expect(el.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toContain('Downloads');
    const items = el.querySelectorAll('pw-downloads-list li');
    expect(items.length).toBe(2);
    expect(items[0]!.textContent).toContain('Downloading');
    expect(items[0]!.textContent).toContain('/s');
    expect(items[1]!.textContent).toContain('Paused · resumable');
    expect(items[1]!.textContent).toContain('Connection reset');
    live.connect();
    (el.querySelector('button[aria-label="Cancel Qwen3-14B-Instruct-Q4_K_M.gguf"]') as HTMLButtonElement).click();
    await settle();
    expect((await backend.downloads())[0]!.state).toBe('cancelled');
    expect(live.downloads()[0]!.state).toBe('cancelled');
    live.disconnect();
  });

  it('refreshes the library when a model download finishes', async () => {
    const backend = new MockBackend();
    const spy = vi.spyOn(backend, 'localModels');
    const { settle, live } = await render(backend);
    const before = spy.mock.calls.length;
    live.connect();
    for (let i = 0; i < 40; i++) backend.tick();
    await settle();
    expect(live.downloads().some((d) => d.state === 'done')).toBe(true);
    expect(spy.mock.calls.length).toBeGreaterThan(before);
    live.disconnect();
  });

  it('shows an empty downloads list', async () => {
    const { el, settle } = await render(new MockBackend({ empty: true }));
    (el.querySelector('#tab-downloads') as HTMLButtonElement).click();
    await settle();
    expect(el.textContent).toContain('No downloads');
  });
});
